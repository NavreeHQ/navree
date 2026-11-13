plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "com.navree.demo"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.navree.demo"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
    }

    // The app has no code of its own: it is the client plus resources.
    enableKotlin = false

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":navree-native"))
}
