plugins {
    java
    application
    id("com.gradleup.shadow") version "8.3.6"
}

group = "io.github.openfluxgate"
version = "0.1.0"

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(21)
    }
}

application {
    mainClass = "io.github.openfluxgate.fluxmirror.Main"
}

repositories {
    mavenCentral()
}

dependencies {
    implementation("com.fasterxml.jackson.core:jackson-databind:2.18.3")
    implementation("org.xerial:sqlite-jdbc:3.47.2.0")
    implementation("org.slf4j:slf4j-api:2.0.17")
    implementation("org.slf4j:slf4j-simple:2.0.17")

    testImplementation(platform("org.junit:junit-bom:5.11.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
}

tasks.test {
    useJUnitPlatform()
}

tasks.shadowJar {
    archiveBaseName = "fluxmirror"
    archiveClassifier = "all"
    archiveVersion = ""
}
