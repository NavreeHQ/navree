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
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        unitTests {
            // The reducer and the design rules never touch the platform; a
            // stray call into it answers a default instead of stopping a test.
            isReturnDefaultValues = true
        }
    }

    sourceSets.named("test") {
        // The example envelopes live beside the project so the tests and the
        // fixture server read the same files.
        resources.directories.add("../fixtures")
    }
}

kotlin {
    compilerOptions {
        // Top app bars, sheets, search and pickers are still marked
        // experimental in Material 3 1.4.
        optIn.add("androidx.compose.material3.ExperimentalMaterial3Api")
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    api(libs.activity.compose)
    implementation(libs.coroutines.android)
    implementation(libs.serialization.json)
    implementation(libs.okhttp)

    testImplementation(libs.junit)
}
