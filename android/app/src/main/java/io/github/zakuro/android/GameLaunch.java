package io.github.zakuro.android;

import android.content.Context;
import android.content.Intent;
import java.io.File;
import java.io.FileInputStream;

/** Both launchers resolve installed files here; external Intents cannot select native code. */
final class GameLaunch {
    static Intent intent(Context context, String id) throws Exception {
        if (id == null || !id.matches("[0-9A-F]{16}")) throw new java.io.IOException("Invalid game ID");
        File version = GameImport.active(context,id);
        GameSettings settings = GameSettings.load(context,id);
        if (settings.flag("aot")) try (java.nio.channels.FileChannel code = new FileInputStream(new File(version,"code/title.so")).getChannel()) {
            NativeLibrary.inspect(code).requireCompatible();
        }
        Intent intent = new Intent(context,Game.class);
        intent.putExtra("game",new File(version,"game").getAbsolutePath());
        intent.putExtra("code",new File(version,"code/title.so").getAbsolutePath());
        intent.putExtra("data",new File(version,"data").getAbsolutePath());
        settings.apply(intent,version);
        return intent;
    }

    /**
     * Where the game opens. With the bottom screen on a second display it is always the main display: started from a
     * launcher on the second screen (the AYN Thor's lower one) it would open there, and the bottom screen could not go
     * to the main display, which takes no presentations ("Second display disconnected").
     */
    static android.os.Bundle options(Context context, Intent intent) {
        String id = intent.getStringExtra("game_id");
        try { if (id == null || !GameSettings.load(context, id).flag("dual_display")) return null; }
        catch (Exception e) { return null; }
        return android.app.ActivityOptions.makeBasic().setLaunchDisplayId(android.view.Display.DEFAULT_DISPLAY).toBundle();
    }

    static void start(Context context, Intent intent) {
        try { context.startActivity(intent, options(context, intent)); }
        catch (SecurityException e) { context.startActivity(intent); } // a display we may not launch on: Android's choice
    }
}
