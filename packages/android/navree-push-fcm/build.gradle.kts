plugins {
    alias(libs.plugins.android.library)
    `maven-publish`
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

    // What is published: the release build, with its sources.
    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}

// com.navree:navree-push-fcm. The version, the folder it is published to, the
// javadoc jar and the rest of the POM are in ../build.gradle.kts.
publishing {
    publications {
        register<MavenPublication>("release") {
            afterEvaluate { from(components["release"]) }
            pom {
                name.set("Navree Native for Android: push through Firebase Cloud Messaging")
                description.set(
                    "Push notifications through Firebase Cloud Messaging for an app on the Android client " +
                        "of Navree Native (com.navree:navree-native).",
                )
            }
        }
    }
}

dependencies {
    implementation(project(":navree-native"))
    implementation(libs.firebase.messaging)
}
