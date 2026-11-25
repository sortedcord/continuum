import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// Rust: continuum-ffi is built with cargo-ndk and its Kotlin bindings are
// generated with uniffi-bindgen (tasks at the bottom of this file).
val rustAbis = listOf("arm64-v8a", "x86_64")
val repoRoot: File = rootDir.parentFile
val rustLibsDir = layout.buildDirectory.dir("rustJniLibs")
val uniffiDir = layout.buildDirectory.dir("generated/uniffi")
val cargoHome = System.getenv("CARGO_HOME") ?: "${System.getProperty("user.home")}/.cargo"
// cargo comes from PATH (rustup); cargo-ndk usually lives in ~/.cargo/bin.
val cargoBin = "cargo"
val rustProfile = providers.gradleProperty("rustProfile").getOrElse("release")
val sdkDir: String? = Properties().also { props ->
    rootProject.file("local.properties").takeIf { it.exists() }?.reader()?.use { props.load(it) }
}.getProperty("sdk.dir") ?: System.getenv("ANDROID_HOME")

android {
    namespace = "io.github.sortedcord.continuum"
    compileSdk {
        version = release(37) { minorApiLevel = 1 }
    }

    defaultConfig {
        applicationId = "io.github.sortedcord.continuum"
        minSdk = 31
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += rustAbis
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            kotlin.directories.add(uniffiDir.get().asFile.path)
            jniLibs.directories.add(rustLibsDir.get().asFile.path)
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.service)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    implementation(libs.camerax.camera2)
    implementation(libs.camerax.lifecycle)
    implementation(libs.camerax.view)
    implementation(libs.zxing.core)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
}


val cargoBuild = tasks.register<Exec>("cargoBuild") {
    description = "Builds libcontinuum_ffi.so for every ABI with cargo-ndk."
    workingDir = repoRoot
    inputs.dir(repoRoot.resolve("crates"))
    inputs.file(repoRoot.resolve("Cargo.lock"))
    outputs.dir(rustLibsDir)
    environment("PATH", "$cargoHome/bin:${System.getenv("PATH")}")
    sdkDir?.let { environment("ANDROID_HOME", it) }
    val args = mutableListOf(cargoBin, "ndk")
    rustAbis.forEach { args += listOf("-t", it) }
    args += listOf("-P", "31", "-o", rustLibsDir.get().asFile.path, "build", "-p", "continuum-ffi")
    if (rustProfile == "release") args += "--release"
    commandLine(args)
}

val uniffiBindings = tasks.register<Exec>("uniffiBindings") {
    description = "Generates Kotlin bindings for continuum-ffi."
    dependsOn(cargoBuild)
    workingDir = repoRoot
    environment("PATH", "$cargoHome/bin:${System.getenv("PATH")}")
    val library = repoRoot.resolve("target/x86_64-linux-android/$rustProfile/libcontinuum_ffi.so")
    inputs.file(library)
    outputs.dir(uniffiDir)
    commandLine(
        cargoBin, "run", "-q", "-p", "uniffi-bindgen", "--",
        "generate", "--library", library.path,
        "--language", "kotlin", "--no-format",
        "--out-dir", uniffiDir.get().asFile.path,
    )
}

tasks.named("preBuild") { dependsOn(uniffiBindings) }
