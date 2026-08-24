# Android APK fixture

This minimal, signed debug application exists only for the Android APK
selection and system-installation acceptance gate. It contains no activity,
permissions, user paths, or private data. The verification script installs and
removes only `org.dlaproject.fixture.apk`.

Build it with the pinned wrapper from the generated Tauri Android project:

```bash
cd src-tauri/gen/android
./gradlew -p ../../../tests/android-apk-fixture assembleDebug
```
