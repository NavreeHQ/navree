plugins {
    alias(libs.plugins.android.library)
}

group = "com.navree"

// Push notifications through Firebase Cloud Messaging, for apps that send
// them. An app without notifications leaves this module out and carries none
// of Firebase.
android {
    namespace = "com.navree.push.fcm"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":navree-native"))
    implementation(libs.firebase.messaging)
}
