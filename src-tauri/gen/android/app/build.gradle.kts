import groovy.json.JsonSlurper
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// `rustls-platform-verifier` (used by reqwest for the on-device model
// download over HTTPS) ships a small Kotlin/JNI component that isn't on
// Maven - it has to be located via `cargo metadata` and pulled in from the
// crate's own bundled maven repo. See its crate docs' "Android" section.
fun rustlsPlatformVerifierMavenDir(): File {
    val cargoManifest = File(rootDir, "../../Cargo.toml").absolutePath
    val metadataJson = providers.exec {
        commandLine(
            "cargo", "metadata", "--format-version", "1",
            "--filter-platform", "aarch64-linux-android",
            "--manifest-path", cargoManifest
        )
    }.standardOutput.asText.get()

    @Suppress("UNCHECKED_CAST")
    val packages = (JsonSlurper().parseText(metadataJson) as Map<String, Any>)["packages"] as List<Map<String, Any>>
    val pkg = packages.first { it["name"] == "rustls-platform-verifier-android" }
    val manifestPath = File(pkg["manifest_path"] as String)
    return File(manifestPath.parentFile, "maven")
}

android {
    compileSdk = 36
    namespace = "com.unbagrnd.app"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.unbagrnd.app"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

repositories {
    maven {
        url = uri(rustlsPlatformVerifierMavenDir())
        // Its POM declares `<packaging>aar</packaging>` - reading the POM
        // (rather than probing for a bare .jar) is what makes Gradle fetch
        // the actual .aar artifact.
        metadataSources { mavenPom() }
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // Pinned (not "latest.release") because that dynamic selector needs a
    // maven-metadata.xml listing, which this crate-bundled repo doesn't
    // publish - only a fixed-version artifact.
    implementation("rustls:rustls-platform-verifier:0.1.1")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")