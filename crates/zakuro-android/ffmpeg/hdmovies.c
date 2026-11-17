/*
 * libexalt_ffmpeg.so: what ExaltDS's HD movie import (crates/zakuro-android/src/hd_import.rs) needs of FFmpeg,
 * behind a few plain C calls, so that the Rust side loads it with dlopen and needs no FFmpeg structures:
 * a video file's size, rate and length, and its pictures from a time on, through a filter chain (deinterlacing,
 * cropping, scaling) as RGB24 or NV12.
 *
 * FFmpeg (https://ffmpeg.org) is LGPL-2.1-or-later and is linked dynamically: libavformat, libavcodec,
 * libavfilter, libswscale and libavutil ship beside this library, built from unmodified source by
 * android/scripts/build-ffmpeg.sh. This file is part of zakuro-android (MIT).
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavformat/avformat.h>
#include <libavutil/opt.h>

#define EXPORT __attribute__((visibility("default")))

typedef struct Video {
    AVFormatContext *format;
    AVCodecContext *codec;
    AVFilterGraph *graph;
    AVFilterContext *source, *sink;
    AVPacket *packet;
    AVFrame *decoded, *filtered;
    int stream, width, height, nv12, ended, flushed;
    int64_t count; /* pictures out of the filters, which time those without a timestamp (raw H.264) */
    int64_t decoded_count; /* and out of the decoder, the same before the filters, which drop untimed pictures */
    double start, half;
} Video;

static void say(char *error, int length, const char *what, int code) {
    char text[128] = "";
    if (code < 0) av_strerror(code, text, sizeof text);
    if (error && length > 0) snprintf(error, (size_t)length, "%s%s%s", what, code < 0 ? ": " : "", text);
}

static int open_input(const char *path, AVFormatContext **format, int *stream, char *error, int length) {
    int code = avformat_open_input(format, path, NULL, NULL);
    if (code < 0) { say(error, length, "can't open the file", code); return code; }
    code = avformat_find_stream_info(*format, NULL);
    if (code < 0) { say(error, length, "can't read the file's streams", code); return code; }
    *stream = av_find_best_stream(*format, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (*stream < 0) { say(error, length, "the file has no video", *stream); return *stream; }
    return 0;
}

/* size, pictures a second, whether interlaced, and length in seconds. 0 when read. */
EXPORT int hd_info(const char *path, int *width, int *height, double *fps, int *interlaced, double *seconds, char *error, int length) {
    AVFormatContext *format = NULL;
    int stream = -1, code = open_input(path, &format, &stream, error, length);
    if (code >= 0) {
        AVStream *s = format->streams[stream];
        *width = s->codecpar->width;
        *height = s->codecpar->height;
        AVRational rate = s->avg_frame_rate.num ? s->avg_frame_rate : s->r_frame_rate;
        *fps = rate.den ? av_q2d(rate) : 0;
        enum AVFieldOrder order = s->codecpar->field_order;
        *interlaced = order == AV_FIELD_TT || order == AV_FIELD_BB || order == AV_FIELD_TB || order == AV_FIELD_BT;
        *seconds = format->duration > 0 ? format->duration / (double)AV_TIME_BASE : 0;
    }
    avformat_close_input(&format);
    return code < 0 ? code : 0;
}

EXPORT void hd_close(Video *v) {
    if (!v) return;
    avfilter_graph_free(&v->graph);
    av_frame_free(&v->decoded);
    av_frame_free(&v->filtered);
    av_packet_free(&v->packet);
    avcodec_free_context(&v->codec);
    avformat_close_input(&v->format);
    free(v);
}

/* the file's pictures from start seconds on, through filters ("" for none, else an FFmpeg filter chain that ends
 * at width x height), as RGB24 or, with nv12, NV12. none when it can't, and why in error. */
EXPORT Video *hd_open(const char *path, double start, const char *filters, int width, int height, int nv12, char *error, int length) {
    Video *v = calloc(1, sizeof *v);
    if (!v) return NULL;
    v->width = width, v->height = height, v->nv12 = nv12, v->start = start;
    int code = open_input(path, &v->format, &v->stream, error, length);
    if (code < 0) { hd_close(v); return NULL; }
    AVStream *s = v->format->streams[v->stream];
    const AVCodec *decoder = avcodec_find_decoder(s->codecpar->codec_id);
    if (!decoder) { say(error, length, "this build can't decode the file's video", 0); hd_close(v); return NULL; }
    v->codec = avcodec_alloc_context3(decoder);
    avcodec_parameters_to_context(v->codec, s->codecpar);
    v->codec->thread_count = 0;
    v->codec->thread_type = FF_THREAD_FRAME | FF_THREAD_SLICE;
    v->codec->pkt_timebase = s->time_base;
    if ((code = avcodec_open2(v->codec, decoder, NULL)) < 0) { say(error, length, "can't start the decoder", code); hd_close(v); return NULL; }
    AVRational rate = s->avg_frame_rate.num ? s->avg_frame_rate : s->r_frame_rate;
    v->half = rate.num ? 0.5 * av_q2d(av_inv_q(rate)) : 0.001;
    if (start > 0) {
        int64_t at = (int64_t)(start / av_q2d(s->time_base));
        if (s->start_time != AV_NOPTS_VALUE) at += s->start_time;
        av_seek_frame(v->format, v->stream, at, AVSEEK_FLAG_BACKWARD);
    }
    /* buffer -> filters -> format -> sink */
    v->graph = avfilter_graph_alloc();
    char args[256];
    AVRational tb = s->time_base, sar = s->codecpar->sample_aspect_ratio.num ? s->codecpar->sample_aspect_ratio : (AVRational){1, 1};
    snprintf(args, sizeof args, "video_size=%dx%d:pix_fmt=%d:time_base=%d/%d:pixel_aspect=%d/%d", v->codec->width, v->codec->height,
             v->codec->pix_fmt, tb.num, tb.den, sar.num, sar.den);
    if ((code = avfilter_graph_create_filter(&v->source, avfilter_get_by_name("buffer"), "in", args, NULL, v->graph)) < 0 ||
        (code = avfilter_graph_create_filter(&v->sink, avfilter_get_by_name("buffersink"), "out", NULL, NULL, v->graph)) < 0) {
        say(error, length, "can't make the filters", code); hd_close(v); return NULL;
    }
    char chain[1024];
    snprintf(chain, sizeof chain, "%s%sformat=%s", filters, filters[0] ? "," : "", nv12 ? "nv12" : "rgb24");
    AVFilterInOut *outputs = avfilter_inout_alloc(), *inputs = avfilter_inout_alloc();
    outputs->name = av_strdup("in"), outputs->filter_ctx = v->source, outputs->pad_idx = 0, outputs->next = NULL;
    inputs->name = av_strdup("out"), inputs->filter_ctx = v->sink, inputs->pad_idx = 0, inputs->next = NULL;
    code = avfilter_graph_parse_ptr(v->graph, chain, &inputs, &outputs, NULL);
    avfilter_inout_free(&inputs);
    avfilter_inout_free(&outputs);
    if (code < 0 || (code = avfilter_graph_config(v->graph, NULL)) < 0) { say(error, length, "the filters don't fit", code); hd_close(v); return NULL; }
    v->packet = av_packet_alloc();
    v->decoded = av_frame_alloc();
    v->filtered = av_frame_alloc();
    return v;
}

/* copies a filtered picture out, row by row, the planes one after the other. */
static void copy_out(Video *v, uint8_t *out) {
    AVFrame *f = v->filtered;
    if (!v->nv12) {
        for (int y = 0; y < v->height; y++) memcpy(out + (size_t)y * v->width * 3, f->data[0] + (size_t)y * f->linesize[0], (size_t)v->width * 3);
        return;
    }
    for (int y = 0; y < v->height; y++) memcpy(out + (size_t)y * v->width, f->data[0] + (size_t)y * f->linesize[0], (size_t)v->width);
    uint8_t *uv = out + (size_t)v->width * v->height;
    for (int y = 0; y < v->height / 2; y++) memcpy(uv + (size_t)y * v->width, f->data[1] + (size_t)y * f->linesize[1], (size_t)v->width);
}

/* the next picture into out (width*height*3 bytes RGB, or width*height*3/2 NV12) and its time in seconds:
 * 1 when there is one, 0 at the end, negative on an error. pictures before the start are dropped. */
EXPORT int hd_next(Video *v, uint8_t *out, double *seconds) {
    AVRational tb = av_buffersink_get_time_base(v->sink);
    for (;;) {
        int code = av_buffersink_get_frame(v->sink, v->filtered);
        if (code >= 0) {
            AVStream *s = v->format->streams[v->stream];
            double at;
            if (v->filtered->pts == AV_NOPTS_VALUE) {
                at = v->count * 2 * v->half;
            } else {
                at = v->filtered->pts * av_q2d(tb);
                if (s->start_time != AV_NOPTS_VALUE) at -= s->start_time * av_q2d(s->time_base);
            }
            v->count++;
            if (v->filtered->width != v->width || v->filtered->height != v->height) {
                av_frame_unref(v->filtered);
                return AVERROR(EINVAL);
            }
            if (at + v->half < v->start) { av_frame_unref(v->filtered); continue; }
            copy_out(v, out);
            *seconds = at;
            av_frame_unref(v->filtered);
            return 1;
        }
        if (code == AVERROR_EOF) return 0;
        if (code != AVERROR(EAGAIN)) return code;
        /* the filters want a picture: decode one */
        code = avcodec_receive_frame(v->codec, v->decoded);
        if (code >= 0) {
            v->decoded->pts = v->decoded->best_effort_timestamp;
            if (v->decoded->pts == AV_NOPTS_VALUE) {
                AVRational tb = v->format->streams[v->stream]->time_base;
                v->decoded->pts = (int64_t)(v->decoded_count * 2 * v->half / av_q2d(tb) + 0.5);
            }
            v->decoded_count++;
            code = av_buffersrc_add_frame_flags(v->source, v->decoded, AV_BUFFERSRC_FLAG_KEEP_REF);
            av_frame_unref(v->decoded);
            if (code < 0) return code;
            continue;
        }
        if (code == AVERROR_EOF) {
            if (!v->flushed) {
                v->flushed = 1;
                if ((code = av_buffersrc_add_frame(v->source, NULL)) < 0) return code;
                continue;
            }
            return 0;
        }
        if (code != AVERROR(EAGAIN)) return code;
        /* the decoder wants a packet; at the file's end it is told so once, drains, then says EOF */
        if (v->ended) return 0;
        code = av_read_frame(v->format, v->packet);
        if (code < 0) {
            v->ended = 1;
            avcodec_send_packet(v->codec, NULL);
            continue;
        }
        if (v->packet->stream_index == v->stream) avcodec_send_packet(v->codec, v->packet);
        av_packet_unref(v->packet);
    }
}
