plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

group = "com.navree"

android {
    namespace = "com.navree.client"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            // The runtime core is built for these two: phones, and the
            // emulator on an Intel or AMD machine.
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        unitTests {
            // The design rules never touch the platform; a stray call into it
            // answers a default instead of stopping a test.
            isReturnDefaultValues = true
        }
    }

    sourceSets.named("test") {
        // The protocol's own example answers, read where the protocol keeps
        // them, and the answers recorded from the store demo.
        resources.directories.add("../../../crates/navree-native/corpus")
        resources.directories.add("../fixtures")
    }

    sourceSets.named("androidTest") {
        // Answers the screen tests serve to the app: the protocol's examples
        // and the store demo's recorded answers.
        assets.directories.add("../../../crates/navree-native/corpus")
        assets.directories.add("../fixtures")
    }
}

kotlin {
    compilerOptions {
        // Top app bars, sheets, search and pickers still need an opt-in in
        // Material 3.
        optIn.add("androidx.compose.material3.ExperimentalMaterial3Api")
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    api(libs.activity.compose)
    implementation(libs.browser)
    implementation(libs.webkit)
    implementation(libs.credentials)
    implementation(libs.credentials.play)
    implementation(libs.googleid)
    implementation(libs.coroutines.android)
    implementation(libs.serialization.json)
    implementation(libs.okhttp)

    testImplementation(libs.junit)
    testImplementation(libs.serialization.json)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test)
    androidTestImplementation(libs.compose.ui.test.manifest)
    androidTestImplementation(libs.test.runner)
    androidTestImplementation(libs.test.core)
    androidTestImplementation(libs.test.junit)
    androidTestImplementation(libs.okhttp.mockwebserver)
}
