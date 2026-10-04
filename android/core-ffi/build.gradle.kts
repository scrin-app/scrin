import org.gradle.api.tasks.Exec

plugins {
    alias(libs.plugins.android.library)
}

// UniFFI library mode: cargo-ndk builds libscrin_ffi.so per ABI; Kotlin bindings are generated
// from the host cdylib's proc-macro metadata (no UDL); JNA loads the library at runtime.
val repoRoot: File = rootProject.projectDir.parentFile
val jniOut = layout.buildDirectory.dir("rustJniLibs")
val bindingsOut = layout.buildDirectory.dir("generated/uniffi/kotlin")
val buildRust = providers.gradleProperty("scrin.buildRust").getOrElse("true").toBoolean()
val ndkVer = "28.2.13676358"

fun abisFor(variant: String): List<String> {
    val key = if (variant == "release") "scrin.rustAbis.release" else "scrin.rustAbis.debug"
    return providers.gradleProperty(key).getOrElse("arm64-v8a").split(',').map { it.trim() }.filter { it.isNotEmpty() }
}

val os = org.gradle.internal.os.OperatingSystem.current()
val hostLibName = when {
    os.isWindows -> "scrin_ffi.dll"
    os.isMacOsX -> "libscrin_ffi.dylib"
    else -> "libscrin_ffi.so"
}
val hostLibFile = repoRoot.resolve("target/debug/$hostLibName")
val rustSources = fileTree(repoRoot) {
    include("crates/**/*.rs", "crates/**/Cargo.toml", "crates/**/build.rs", "crates/scrin-ffi/uniffi.toml", "proto/**/*.proto", "Cargo.toml", "Cargo.lock")
    exclude("target/**")
}

fun Exec.ndkEnv() {
    val sdk = System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
        ?: rootProject.file("local.properties").takeIf { it.exists() }?.readLines()
            ?.firstOrNull { it.startsWith("sdk.dir=") }?.substringAfter("=")?.replace("\\:", ":")?.replace("\\\\", "\\")
        ?: error("ANDROID_HOME not set and no local.properties sdk.dir")
    environment("ANDROID_NDK_HOME", File(sdk, "ndk/$ndkVer").absolutePath)
}

android {
    namespace = "ro.dragoscatalin.scrin.ffi"
    compileSdk = 37
    ndkVersion = ndkVer
    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }
    buildTypes {
        release { isMinifyEnabled = false }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    sourceSets {
        getByName("main") {
            jniLibs.directories.add(jniOut.get().asFile.absolutePath)
            kotlin.directories.add(bindingsOut.get().asFile.absolutePath)
        }
    }
    lint {
        abortOnError = true
        warningsAsErrors = true
        // Generated UniFFI bindings are not ours to lint.
        ignoreTestSources = true
        checkGeneratedSources = false
    }
    testOptions {
        unitTests.all {
            // The generated bindings run against the host cdylib (built by cargoBuildHostFfi).
            it.systemProperty("jna.library.path", hostLibFile.parentFile.absolutePath)
            it.dependsOn("cargoBuildHostFfi")
            it.inputs.file(hostLibFile)
        }
    }
}

dependencies {
    implementation(libs.jna) { artifact { type = "aar" } }
    implementation(libs.androidx.annotation) // @RequiresApi in UniFFI's android_cleaner output
    testImplementation(libs.junit)
    testImplementation(libs.jna) // desktop jar: carries the host jnidispatch the AAR lacks
}

fun cargoNdk(name: String, variant: String) = tasks.register<Exec>(name) {
    group = "rust"
    description = "cargo ndk build of scrin-ffi ($variant)"
    enabled = buildRust
    workingDir = repoRoot
    ndkEnv()
    val abis = abisFor(variant).flatMap { listOf("-t", it) }
    val profile = if (variant == "release") listOf("--release") else emptyList()
    commandLine(listOf("cargo", "ndk", "-o", jniOut.get().asFile.absolutePath, "--platform", "26") + abis + listOf("build", "-p", "scrin-ffi") + profile)
    inputs.files(rustSources)
    inputs.property("abis", abis)
    outputs.dir(jniOut)
}

val cargoNdkDebug = cargoNdk("cargoNdkDebug", "debug")
val cargoNdkRelease = cargoNdk("cargoNdkRelease", "release")

val hostLib = tasks.register<Exec>("cargoBuildHostFfi") {
    group = "rust"
    description = "Host build of scrin-ffi (bindgen metadata + JVM unit tests)"
    enabled = buildRust
    workingDir = repoRoot
    commandLine("cargo", "build", "-p", "scrin-ffi", "--features", "cli")
    inputs.files(rustSources)
    outputs.file(hostLibFile)
}

val uniffiBindgen = tasks.register<Exec>("uniffiBindgen") {
    group = "rust"
    description = "Generate Kotlin bindings for scrin-ffi (library mode)"
    enabled = buildRust
    dependsOn(hostLib)
    workingDir = repoRoot
    // uniffi >= 0.32: `--config` takes a *global* file ([defaults]/[crates.x]); the crate's own
    // crates/scrin-ffi/uniffi.toml is found through cargo metadata, so no flag is needed.
    commandLine(
        "cargo", "run", "-p", "scrin-ffi", "--features", "cli", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", "target/debug/$hostLibName", "--language", "kotlin",
        "--out-dir", bindingsOut.get().asFile.absolutePath, "--no-format",
    )
    inputs.file(hostLibFile)
    inputs.file(repoRoot.resolve("crates/scrin-ffi/uniffi.toml"))
    outputs.dir(bindingsOut)
}

// Debug and release share one jniLibs dir; wire each variant's merge task to its own cargo task.
tasks.matching { it.name == "mergeDebugJniLibFolders" }.configureEach { dependsOn(cargoNdkDebug) }
tasks.matching { it.name == "mergeReleaseJniLibFolders" }.configureEach { dependsOn(cargoNdkRelease) }
tasks.matching {
    (it.name.startsWith("compile") && it.name.contains("Kotlin")) ||
        (it.name.startsWith("extract") && it.name.endsWith("Annotations")) ||
        it.name.startsWith("lintAnalyze") || it.name.startsWith("generate") && it.name.endsWith("LintModel")
}.configureEach { dependsOn(uniffiBindgen) }
