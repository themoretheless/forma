#!/usr/bin/env sh
set -eu

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@17}"
export ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.0.12077973}"
BUILD_TOOLS="$ANDROID_HOME/build-tools/35.0.0"
PLATFORM_JAR="$ANDROID_HOME/platforms/android-35/android.jar"
ADB="$ANDROID_HOME/platform-tools/adb"
EMULATOR="$ANDROID_HOME/emulator/emulator"

BACKEND_MODE=0
case "${1:-}" in
  --egui|egui) BACKEND_MODE=1 ;;
  --svet|svet) BACKEND_MODE=2 ;;
esac

echo "[1/5] Сборка Rust libforma.so (arm64-v8a)..."
(cd "$ROOT_DIR/vector-ui" && ANDROID_NDK_HOME="$ANDROID_NDK_HOME" cargo ndk -t arm64-v8a build --release --offline --lib)

WORK_DIR="$ROOT_DIR/vector-ui/target/android-apk"
rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR/src/dev/forma/app" "$WORK_DIR/classes" "$WORK_DIR/lib/arm64-v8a" "$WORK_DIR/dex"

cat > "$WORK_DIR/AndroidManifest.xml" << 'EOF'
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="dev.forma.app"
    android:versionCode="1"
    android:versionName="0.1.0">
    <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="35" />
    <application
        android:label="Forma UI"
        android:theme="@android:style/Theme.DeviceDefault.NoActionBar">
        <activity
            android:name="dev.forma.app.MainActivity"
            android:exported="true"
            android:configChanges="orientation|screenSize|screenLayout|keyboardHidden">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
EOF

cat > "$WORK_DIR/src/dev/forma/app/MainActivity.java" << 'EOF'
package dev.forma.app;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.os.Bundle;
import android.view.Choreographer;
import android.view.MotionEvent;
import android.view.View;
import android.widget.FrameLayout;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

public class MainActivity extends Activity {
    static {
        System.loadLibrary("forma");
    }

    public static native void nativeInit(int backendMode);
    public static native int nativePointer(float x, float y, int kind);
    public static native int nativeToggleBackend();
    public static native void nativeRender(ByteBuffer buffer, int width, int height, float scale, float dtMs);

    private FormaSurfaceView formaView;
    private String currentBackend = "Выбор: [1] egui / [2] svet";
    private int clickCount = 0;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        int mode = 0;
        Intent intent = getIntent();
        if (intent != null && intent.hasExtra("backend")) {
            String b = intent.getStringExtra("backend");
            if ("egui".equalsIgnoreCase(b)) {
                mode = 1;
                currentBackend = "egui (Immediate-Mode)";
            } else if ("svet".equalsIgnoreCase(b)) {
                mode = 2;
                currentBackend = "svet (Reactive GPU/UMA)";
            }
        }
        nativeInit(mode);
        formaView = new FormaSurfaceView(this);
        setContentView(formaView);
    }

    private class FormaSurfaceView extends View implements Choreographer.FrameCallback {
        private Bitmap bitmap;
        private ByteBuffer pixelBuffer;
        private long lastFrameNs = System.nanoTime();
        private final Paint hudPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint pillPaint = new Paint(Paint.ANTI_ALIAS_FLAG);

        public FormaSurfaceView(Context ctx) {
            super(ctx);
            hudPaint.setColor(0xFFE6EDF7);
            hudPaint.setTextSize(34f);
            pillPaint.setColor(0xCC1E2636);
            Choreographer.getInstance().postFrameCallback(this);
        }

        @Override
        public void doFrame(long frameTimeNanos) {
            invalidate();
            Choreographer.getInstance().postFrameCallback(this);
        }

        @Override
        protected void onDraw(Canvas canvas) {
            int w = getWidth();
            int h = getHeight();
            if (w <= 0 || h <= 0) return;
            if (bitmap == null || bitmap.getWidth() != w || bitmap.getHeight() != h) {
                bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888);
                pixelBuffer = ByteBuffer.allocateDirect(w * h * 4).order(ByteOrder.nativeOrder());
            }
            long now = System.nanoTime();
            float dtMs = (now - lastFrameNs) / 1_000_000f;
            lastFrameNs = now;
            float density = getResources().getDisplayMetrics().density;

            pixelBuffer.rewind();
            nativeRender(pixelBuffer, w, h, density, dtMs);
            pixelBuffer.rewind();
            bitmap.copyPixelsFromBuffer(pixelBuffer);
            canvas.drawBitmap(bitmap, 0f, 0f, null);

            // Bottom HUD bar with backend toggle button
            float pillTop = h - 84f * density;
            float pillHeight = 36f * density;
            canvas.drawRoundRect(16f * density, pillTop, w - 16f * density, pillTop + pillHeight, 12f * density, 12f * density, pillPaint);
            hudPaint.setTextSize(12f * density);
            canvas.drawText("Forma Android · " + currentBackend + " · кликов: " + clickCount, 26f * density, pillTop + 23f * density, hudPaint);
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            float density = getResources().getDisplayMetrics().density;
            float x = event.getX() / density;
            float y = event.getY() / density;
            int h = getHeight();
            int action = event.getActionMasked();

            if (action == MotionEvent.ACTION_DOWN && event.getY() >= h - 84f * density && event.getY() <= h - 48f * density) {
                int next = nativeToggleBackend();
                currentBackend = (next == 1) ? "egui (Immediate-Mode)" : "svet (Reactive GPU/UMA)";
                invalidate();
                return true;
            }

            if (action == MotionEvent.ACTION_DOWN) {
                nativePointer(x, y, 0);
                nativePointer(x, y, 1);
            } else if (action == MotionEvent.ACTION_MOVE) {
                nativePointer(x, y, 0);
            } else if (action == MotionEvent.ACTION_UP) {
                int res = nativePointer(x, y, 2);
                if (res == 101) {
                    currentBackend = "egui (Immediate-Mode)";
                } else if (res == 102) {
                    currentBackend = "svet (Reactive GPU/UMA)";
                } else if (res > 0) {
                    clickCount = res;
                }
            } else if (action == MotionEvent.ACTION_CANCEL) {
                nativePointer(-1f, -1f, 3);
            }
            invalidate();
            return true;
        }
    }
}
EOF

echo "[2/5] Компиляция Java и D8 DEX..."
"$JAVA_HOME/bin/javac" --release 11 -cp "$PLATFORM_JAR" -d "$WORK_DIR/classes" "$WORK_DIR/src/dev/forma/app/MainActivity.java"
"$BUILD_TOOLS/d8" --lib "$PLATFORM_JAR" --output "$WORK_DIR/dex" "$WORK_DIR/classes/dev/forma/app/"*.class

echo "[3/5] Упаковка APK (aapt2 + libforma.so)..."
"$BUILD_TOOLS/aapt2" link -o "$WORK_DIR/unaligned.apk" -I "$PLATFORM_JAR" --manifest "$WORK_DIR/AndroidManifest.xml"
cp "$ROOT_DIR/vector-ui/target/aarch64-linux-android/release/libforma.so" "$WORK_DIR/lib/arm64-v8a/libforma.so"
(
  cd "$WORK_DIR"
  cp "$WORK_DIR/dex/classes.dex" "$WORK_DIR/classes.dex"
  zip -q -u unaligned.apk classes.dex
  zip -q -0 -u unaligned.apk lib/arm64-v8a/libforma.so
)

echo "[4/5] Выравнивание и подпись APK..."
"$BUILD_TOOLS/zipalign" -f -p 4 "$WORK_DIR/unaligned.apk" "$WORK_DIR/aligned.apk"
KEYSTORE="$ROOT_DIR/vector-ui/target/debug.keystore"
if [ ! -f "$KEYSTORE" ]; then
  "$JAVA_HOME/bin/keytool" -genkeypair -v -keystore "$KEYSTORE" -storepass android -alias androiddebugkey -keypass android -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null 2>&1
fi
"$BUILD_TOOLS/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android --key-pass pass:android --out "$WORK_DIR/forma-debug.apk" "$WORK_DIR/aligned.apk"

echo "[5/5] Проверка эмулятора и запуск APK..."
if ! "$ADB" devices | grep -q "device$"; then
  nohup "$EMULATOR" -avd Forma_Pixel_9 -gpu host >/dev/null 2>&1 &
  "$ADB" wait-for-device
fi
while [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" != "1" ]; do
  sleep 1
done
"$ADB" uninstall dev.forma.app >/dev/null 2>&1 || true
"$ADB" install -r "$WORK_DIR/forma-debug.apk"
if [ "$BACKEND_MODE" = "1" ]; then
  "$ADB" shell am start -S -n dev.forma.app/.MainActivity --es backend egui
elif [ "$BACKEND_MODE" = "2" ]; then
  "$ADB" shell am start -S -n dev.forma.app/.MainActivity --es backend svet
else
  "$ADB" shell am start -S -n dev.forma.app/.MainActivity
fi
echo "Готово! Forma запущена в Android Emulator."
