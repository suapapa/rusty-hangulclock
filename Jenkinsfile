// Manual OTA release pipeline for rusty-hangulclock.
//
// Flow:
//   1) Resolve SW_VERSION (param or FWVER)
//   2) ./make_ota_bins.sh  → release/*.bin (HW rev 3, 4)
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

@Library('homin-jenkins-shared-lib') _

pipeline {
    // agent any
    agent { label 'esp-rs'}

    options {
        timeout(time: 2, unit: 'HOURS')
        disableConcurrentBuilds()
        ansiColor('xterm')
    }

    parameters {
        string(
            name: 'SW_VERSION',
            defaultValue: '',
            description: 'OTA software version. Leave empty to use FWVER from the repo.'
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
                }
            }
        }

        stage('Pre-flight') {
            steps {
                sh '''
                    set -eu pipefail
                    echo "=== Toolchain Verification ==="
                    cargo --version
                    rustc --version
                    command -v cargo-espflash
                    command -v sccache && sccache --version || echo "WARN: sccache not found"
                    git --version
                    test -n "${HOMIN_DEV_TOKEN:-}" || { echo "ERROR: HOMIN_DEV_TOKEN is not set"; exit 1; }
                    mkdir -p release
                    sccache --zero-stats || true
                '''
            }
        }

        stage('Build OTA bins') {
            steps {
                sh '''
                    set -eu pipefail
                    if [ -f "${HOME}/export-esp.sh" ]; then
                        # shellcheck disable=SC1090
                        source "${HOME}/export-esp.sh"
                    fi
                    chmod +x ./make_ota_bins.sh
                    ./make_ota_bins.sh -v "${SW_VERSION}"
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
                        sourceDir: 'release'
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
                        hwRevisions: ['3', '4'],
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
