// OTA release pipeline for rusty-hangulclock.
//
// Triggers:
//   - Manual "Build with Parameters"
//   - Git tag push (e.g. v35) via Multibranch / GitHub webhook
//
// Flow:
//   1) Resolve SW_VERSION (param or FWVER) and selected HW revisions
//   1b) If build is from tag vN, require FWVER == N (fail otherwise)
//   2) ./make_ota_bins.sh  → release/*.bin (selected HW rev 3 and/or 4)
//   3) espRsCdnPush        → clone suapapa/homin-dev_asset, commit, push
//   4) espRsOtaRegister    → POST metadata to hangulclock OTA API
//
// Agent prerequisites (preinstalled / env):
//   rust + esp-rs (espup), ~/export-esp.sh, cargo-espflash
//   git SSH access to suapapa/homin-dev_asset
//   HOMIN_DEV_TOKEN in the agent environment
//
// Jenkins setup:
//   Manage Jenkins → System → Global Pipeline Libraries
//     Name: homin-jenkins-shared-lib  (must match @Library below)
//     Source: https://github.com/suapapa/jenkins-shared-lib
//
// Tag-push builds (Multibranch Pipeline recommended):
//   Branch Sources → GitHub → Behaviors:
//     - Discover tags
//   Build Strategies:
//     - Tags (build tags discovered from the repository)
//   GitHub webhook (push events) must reach Jenkins so tag creates/builds run.
//   Tag naming for FWVER check: v<digits> only (e.g. v35 → FWVER must be 35).

@Library('homin-jenkins-shared-lib') _

pipeline {
    // agent any
    agent { label 'esp-rs'}

    options {
        timeout(time: 2, unit: 'HOURS')
        disableConcurrentBuilds()
        ansiColor('xterm')
    }

    // githubPush() lets the GitHub plugin wake Multibranch / Pipeline jobs on
    // webhook push events (including tags when Discover tags is enabled).
    triggers {
        githubPush()
    }

    parameters {
        string(
            name: 'SW_VERSION',
            defaultValue: '',
            description: 'OTA software version. Leave empty to use FWVER from the repo.'
        )
        booleanParam(
            name: 'HW_REV_3',
            defaultValue: true,
            description: 'Build and release for HW revision 3.'
        )
        booleanParam(
            name: 'HW_REV_4',
            defaultValue: true,
            description: 'Build and release for HW revision 4.'
        )
        booleanParam(
            name: 'SKIP_CDN',
            defaultValue: false,
            description: 'Build + archive only; do not push to homin-dev_asset.'
        )
        booleanParam(
            name: 'SKIP_OTA_API',
            defaultValue: false,
            description: 'Skip registering binaries with the OTA update API.'
        )
    }

    environment {
        ASSET_REPO = 'suapapa/homin-dev_asset'
        ASSET_SUBDIR = 'asset/rusty-hangulclock_fw'
        OTA_API_URL = 'https://hangulclock.homin.dev/v1/update'
        DOWNLOAD_URL_BASE = 'https://asset.homin.dev/rusty-hangulclock_fw/'
        OTA_BIN_PREFIX = 'rusty-hangulclock'

        // rustup install is not on non-login shell PATH by default
        PATH = "${HOME}/.cargo/bin:${env.PATH}"
        RUSTC_WRAPPER = 'sccache'
        SCCACHE_DIR = "${WORKSPACE}/.sccache"
        CARGO_TERM_COLOR = 'always'
        HOMIN_DEV_TOKEN = credentials('HOMIN_DEV_TOKEN')
    }

    stages {
        stage('Checkout & Version') {
            steps {
                checkout scm
                script {
                    espRsResolveFwVer()
                    validateTagMatchesFwver()

                    def hwRevs = []
                    if (params.HW_REV_3) { hwRevs << '3' }
                    if (params.HW_REV_4) { hwRevs << '4' }
                    if (hwRevs.isEmpty()) {
                        error('Select at least one HW revision (HW_REV_3 and/or HW_REV_4).')
                    }
                    env.HW_REVISIONS = hwRevs.join(' ')
                    echo "HW revisions: ${env.HW_REVISIONS}"
                }
            }
        }

        stage('Pre-flight') {
            steps {
                // Shebang required: Jenkins defaults to /bin/sh (dash), not bash.
                sh '''#!/bin/bash
                    set -euo pipefail
                    echo "=== Toolchain Verification ==="
                    cargo --version
                    rustc --version
                    command -v cargo-espflash
                    if command -v sccache >/dev/null 2>&1; then
                        sccache --version
                    else
                        echo "WARN: sccache not found"
                    fi
                    git --version
                    if [ -z "${HOMIN_DEV_TOKEN:-}" ]; then
                        echo "ERROR: HOMIN_DEV_TOKEN is not set"
                        exit 1
                    fi
                    mkdir -p release
                    sccache --zero-stats || true
                '''
            }
        }

        stage('Build OTA bins') {
            steps {
                // Shebang required: Jenkins defaults to /bin/sh (dash), not bash.
                sh '''#!/bin/bash
                    set -euo pipefail
                    if [ -f "${HOME}/export-esp.sh" ]; then
                        # shellcheck disable=SC1090
                        # Prefer `.` over `source` (works in bash and POSIX sh)
                        . "${HOME}/export-esp.sh"
                    fi
                    chmod +x ./make_ota_bins.sh
                    ./make_ota_bins.sh -v "${SW_VERSION}" -r "${HW_REVISIONS}"
                    echo "=== Built artifacts ==="
                    ls -la release/*_"${SW_VERSION}"_*.bin
                '''
            }
        }

        stage('Archive') {
            steps {
                archiveArtifacts artifacts: "release/*_${env.SW_VERSION}_*.bin", fingerprint: true
                script {
                    espRsSccacheStats()
                }
            }
        }

        stage('CDN push') {
            when {
                expression { return !params.SKIP_CDN }
            }
            steps {
                script {
                    espRsCdnPush(
                        assetRepo: env.ASSET_REPO,
                        assetSubdir: env.ASSET_SUBDIR,
                        version: env.SW_VERSION,
                        sourceDir: 'release',
                        keepCount: 6 // 최종 (두개의 하드웨어 리비젼의) 3개 버전의 아티팩트들만 남김
                    )
                }
            }
        }

        stage('OTA API register') {
            when {
                expression { return !params.SKIP_OTA_API }
            }
            steps {
                script {
                    espRsOtaRegister(
                        version: env.SW_VERSION,
                        hwRevisions: env.HW_REVISIONS.split(' ').toList(),
                        sourceDir: 'release',
                        binPrefix: env.OTA_BIN_PREFIX,
                        apiUrl: env.OTA_API_URL,
                        downloadUrlBase: env.DOWNLOAD_URL_BASE,
                        uploadScript: 'release/upload_update.sh'
                    )
                }
            }
        }
    }

    post {
        always {
            cleanWs(
                deleteDirs: true,
                notFailBuild: true,
                patterns: [
                    [pattern: 'target/**', type: 'INCLUDE'],
                    [pattern: 'cdn-asset/**', type: 'INCLUDE']
                ]
            )
        }
        success {
            echo "OTA release ${env.SW_VERSION} completed."
        }
        failure {
            echo 'OTA pipeline failed. Check console logs.'
        }
    }
}

/**
 * When this build is from a release tag like v35, require FWVER == "35".
 * Non-matching tags (e.g. v0.4.25) and non-tag builds skip this check.
 */
def validateTagMatchesFwver(String fwverFile = 'FWVER') {
    def tagName = env.TAG_NAME?.toString()?.trim()
    if (!tagName) {
        // Multibranch tag builds set TAG_NAME; fall back to exact tag at HEAD
        // so classic Pipeline checkouts of a tag still validate.
        tagName = sh(
            script: 'git describe --exact-match --tags HEAD 2>/dev/null || true',
            returnStdout: true
        ).trim()
    }
    if (!tagName) {
        echo 'No git tag on this build; skipping FWVER ↔ tag check.'
        return
    }

    if (!(tagName ==~ /^v\d+$/)) {
        echo "Git tag '${tagName}' is not v<digits>; skipping FWVER ↔ tag check."
        return
    }
    def tagVer = tagName.substring(1)

    if (!fileExists(fwverFile)) {
        error("Git tag '${tagName}' requires ${fwverFile}=${tagVer}, but ${fwverFile} is missing.")
    }
    def fwver = readFile(fwverFile).trim()
    if (fwver != tagVer) {
        error(
            "Git tag '${tagName}' expects ${fwverFile}=${tagVer}, " +
            "but ${fwverFile} contains '${fwver}'. " +
            "Update ${fwverFile} to ${tagVer} (or retag) before releasing."
        )
    }
    echo "Git tag '${tagName}' matches ${fwverFile}=${fwver}."
}
