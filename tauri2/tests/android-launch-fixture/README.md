# Android launch fixture

This signed debug application exists only for the reviewed Android application
association and launch acceptance gate. Unlike `android-apk-fixture`, it has one
visible launcher activity so the gate can prove explicit launch and recovery.
It requests no permissions and contains no user paths or private data.

Build it with the pinned wrapper from the generated Tauri Android project:

```bash
cd src-tauri/gen/android
./gradlew -p ../../../tests/android-launch-fixture assembleDebug
```
