import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

abstract class CopyTranscriptsTask : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val inputDir: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun copy() {
        val target = outputDir.get().asFile.resolve("transcripts")
        target.deleteRecursively()
        inputDir.get().asFile.copyRecursively(target, overwrite = true)
    }
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

androidComponents {
    onVariants { variant ->
        val androidTest = variant.androidTest ?: return@onVariants
        val taskName = "copy${variant.name.replaceFirstChar { it.uppercase() }}AndroidTestTranscripts"
        val copyTask = tasks.register<CopyTranscriptsTask>(taskName) {
            inputDir.set(layout.projectDirectory.dir("../../../crates/navree-native-core/transcripts"))
        }
        androidTest.sources.assets?.addGeneratedSourceDirectory(copyTask, CopyTranscriptsTask::outputDir)
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
    implementation(libs.exifinterface)
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
