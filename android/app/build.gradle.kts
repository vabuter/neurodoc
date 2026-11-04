plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// ======== Версия приложения ========
// Единственный источник — git-тег вида vX.Y.Z на коммите в main, тот же, что и
// у Windows-версии. Хардкодить версию здесь нельзя: платформы разъедутся.
// Сборка без git не падает, а честно показывает 0.0.0-nogit.
// providers.exec, а не ProcessBuilder: только он совместим с configuration cache.
fun git(vararg args: String): String = runCatching {
    providers.exec {
        commandLine("git", *args)
        isIgnoreExitValue = true // на репозитории без тегов describe возвращает не ноль
    }.standardOutput.asText.get().trim()
}.getOrDefault("")

val gitTag = git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*")
val gitFull = git("describe", "--tags", "--dirty", "--always", "--match", "v[0-9]*")

// versionCode обязан расти между установками, иначе APK не встанет поверх прежнего
val appVersionCode = Regex("""^v(\d+)\.(\d+)\.(\d+)""").find(gitTag)
    ?.destructured
    ?.let { (major, minor, patch) -> major.toInt() * 10000 + minor.toInt() * 100 + patch.toInt() }
    ?.coerceAtLeast(1)
    ?: 1

val appVersionName = when {
    gitTag.startsWith("v") -> gitFull.removePrefix("v")
    gitFull.isNotEmpty() -> "0.0.0+g$gitFull"
    else -> "0.0.0-nogit"
}

android {
    namespace = "today.vab.neurodoceditor"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "today.vab.neurodoceditor"
        minSdk = 30
        targetSdk = 36
        versionCode = appVersionCode
        versionName = appVersionName

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }
    buildFeatures {
        compose = true
        buildConfig = true // BuildConfig.VERSION_NAME для показа версии в настройках
    }
    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
            // эклипсовские артефакты внутри jgit-jar конфликтуют при merge ресурсов;
            // META-INF/services при этом не трогать — там транспорты JGit
            excludes += "/META-INF/DEPENDENCIES"
            excludes += "/META-INF/LICENSE*"
            excludes += "/META-INF/NOTICE*"
            excludes += "/META-INF/eclipse.inf"
            excludes += "/about.html"
            excludes += "/plugin.properties"
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.jgit)
    implementation(libs.slf4j.nop) // глушит "Failed to load StaticLoggerBinder" от JGit
    implementation(libs.kotlinx.coroutines.android)
    debugImplementation(libs.androidx.compose.ui.tooling)
    testImplementation(libs.junit)
}
