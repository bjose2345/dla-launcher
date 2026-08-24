plugins {
    id("com.android.application")
}

android {
    namespace = "org.dlaproject.fixture.apk"
    compileSdk = 36

    defaultConfig {
        applicationId = "org.dlaproject.fixture.apk"
        minSdk = 24
        targetSdk = 36
        versionCode = 7
        versionName = "1.2.3"
    }
}
