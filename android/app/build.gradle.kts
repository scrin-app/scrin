plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

val scrinVersionName = providers.gradleProperty("scrin.versionName").get()
val scrinVersionCode = providers.gradleProperty("scrin.versionCode").get().toInt()

android {
    namespace = "ro.dragoscatalin.scrin"
    compileSdk = 37

    defaultConfig {
        applicationId = "ro.dragoscatalin.scrin"
        minSdk = 26
        targetSdk = 37
        versionCode = scrinVersionCode
        versionName = scrinVersionName
        // Only ABIs the Rust core is built for; drops JNA's other blobs.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    androidResources {
        localeFilters += listOf("en", "ro")
    }

    flavorDimensions += "distribution"
    productFlavors {
        create("gms") {
            dimension = "distribution"
            isDefault = true
        }
        create("foss") {
            dimension = "distribution"
            applicationIdSuffix = ".foss"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
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
    packaging {
        resources.excludes += setOf("META-INF/*.kotlin_module", "META-INF/LICENSE*", "META-INF/AL2.0", "META-INF/LGPL2.1")
        jniLibs.useLegacyPackaging = false
        // JNA ships libjnidispatch.so already stripped; skip the no-op strip attempt (and its notice).
        jniLibs.keepDebugSymbols += "**/libjnidispatch.so"
    }
    lint {
        abortOnError = true
        warningsAsErrors = true
        checkDependencies = false
        ignoreTestSources = true
        // Versions are pinned deliberately in libs.versions.toml (latest stable on purpose-date);
        // "newer version available" is a network-dependent, non-reproducible check.
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion")
    }
    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

dependencies {
    implementation(project(":core-ffi"))

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.foundation)
    implementation(libs.compose.animation)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons.core)

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.service)
    implementation(libs.androidx.navigation3.runtime)
    implementation(libs.androidx.navigation3.ui)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}

// The foss flavour must not resolve any Google Play Services / Firebase / ML Kit artifact.
val checkFossNoGms = tasks.register("checkFossNoGms") {
    group = "verification"
    description = "Fails if fossReleaseRuntimeClasspath contains GMS, Firebase or ML Kit."
    val ids = configurations.named("fossReleaseRuntimeClasspath").flatMap { c ->
        c.incoming.resolutionResult.rootComponent.map { root ->
            val seen = mutableSetOf<String>()
            fun walk(r: org.gradle.api.artifacts.result.ResolvedComponentResult) {
                r.dependencies.filterIsInstance<org.gradle.api.artifacts.result.ResolvedDependencyResult>().forEach { d ->
                    if (seen.add(d.selected.id.displayName)) walk(d.selected)
                }
            }
            walk(root)
            seen.toList()
        }
    }
    inputs.property("ids", ids)
    val stamp = layout.buildDirectory.file("reports/checkFossNoGms.txt")
    outputs.file(stamp)
    doLast {
        val banned = listOf("com.google.android.gms:", "com.google.firebase:", "com.google.mlkit:", "com.google.android.odml:")
        val bad = ids.get().filter { id -> banned.any { id.startsWith(it) } }
        if (bad.isNotEmpty()) throw GradleException("foss pulls non-free Google deps:\n" + bad.joinToString("\n"))
        val msg = "foss runtime classpath: ${ids.get().size} components, 0 GMS/Firebase/ML Kit"
        stamp.get().asFile.writeText(msg + "\n")
        println(msg)
    }
}
tasks.matching { it.name == "check" }.configureEach { dependsOn(checkFossNoGms) }
