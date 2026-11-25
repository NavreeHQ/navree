import org.gradle.api.publish.PublishingExtension
import org.gradle.api.publish.maven.MavenPublication
import org.gradle.api.tasks.bundling.Jar

plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}

// What the modules published to Maven Central share: the version, the folder
// they are published to, the javadoc jar, and the part of the POM that says
// whose the library is. A module that is published applies `maven-publish` and
// says its own name and description in its build file.
subprojects {
    val module = name

    // A release gives its version (`-PnavreeVersion=1.6.0`); a build without
    // one makes 0.0.0.
    version = providers.gradleProperty("navreeVersion").getOrElse("0.0.0")

    pluginManager.withPlugin("maven-publish") {
        // Maven Central takes no library without a javadoc jar. The client's
        // documentation is the KDoc in its sources jar, so this one holds a
        // page that says where to read.
        val javadocJar = tasks.register<Jar>("javadocJar") {
            archiveBaseName.set(module)
            archiveClassifier.set("javadoc")
            destinationDirectory.set(layout.buildDirectory.dir("libs"))
            from(rootProject.layout.projectDirectory.file("gradle/javadoc/README.md"))
        }

        extensions.configure<PublishingExtension> {
            repositories {
                // `publishAllPublicationsToFolderRepository` writes a Maven
                // repository into build/maven: what scripts/maven-package.sh
                // checks and packs, and scripts/maven-central.sh signs and
                // uploads.
                maven {
                    name = "folder"
                    url = rootProject.layout.buildDirectory.dir("maven").get().asFile.toURI()
                }
            }
            publications.withType<MavenPublication>().configureEach {
                artifact(javadocJar)
                // The POM and the Gradle metadata name every dependency at
                // the version the release build resolved, the ones a BOM
                // gives a version to among them: Maven Central is given no
                // dependency without a version.
                versionMapping {
                    usage("java-api") { fromResolutionOf("releaseRuntimeClasspath") }
                    usage("java-runtime") { fromResolutionOf("releaseRuntimeClasspath") }
                }
                pom {
                    url.set("https://navree.com")
                    licenses {
                        license {
                            name.set("MIT")
                            url.set("https://github.com/NavreeHQ/navree/blob/main/LICENSE-MIT")
                        }
                        license {
                            name.set("Apache-2.0")
                            url.set("https://github.com/NavreeHQ/navree/blob/main/LICENSE-APACHE")
                        }
                    }
                    developers {
                        developer {
                            name.set("Amritpal Singh")
                            organization.set("Navree")
                            organizationUrl.set("https://navree.com")
                        }
                    }
                    scm {
                        connection.set("scm:git:https://github.com/NavreeHQ/navree.git")
                        developerConnection.set("scm:git:ssh://git@github.com/NavreeHQ/navree.git")
                        url.set("https://github.com/NavreeHQ/navree")
                    }
                }
            }
        }
    }
}
