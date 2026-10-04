plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

val scrinVersionName = providers.gradleProperty("scrin.versionName").get()
val scrinVersionCode = providers.gradleProperty("scrin.versionCode").get().toInt()

// Android TV client (leanback launcher). Minimal for now: D-pad focusable connect form.
android {
    namespace = "ro.dragoscatalin.scrin.tv"
    compileSdk = 37

    defaultConfig {
        applicationId = "ro.dragoscatalin.scrin"
        minSdk = 26
        targetSdk = 37
        // TV builds live in their own versionCode range (Play multi-APK rule).
        versionCode = 300_000_000 + scrinVersionCode
        versionName = scrinVersionName
    }
    androidResources {
        localeFilters += listOf("en", "ro")
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
    }
    lint {
        abortOnError = true
        warningsAsErrors = true
        checkDependencies = false
        ignoreTestSources = true
        // IconMissingDensityFolder: the only bitmap is the leanback banner, which Android TV
        // renders at xhdpi (320x180 dp spec); the brand pack ships exactly xhdpi + xxxhdpi
        // (4K), and mdpi/hdpi/xxhdpi TV panels do not exist. The launcher icon is a vector.
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion", "IconMissingDensityFolder")
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.tv.material)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
}
