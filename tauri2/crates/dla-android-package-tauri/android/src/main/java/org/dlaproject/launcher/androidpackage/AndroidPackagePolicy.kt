package org.dlaproject.launcher.androidpackage

internal object AndroidPackagePolicy {
  fun blockReason(
    packageName: String,
    currentPackageName: String,
    split: Boolean,
    minimumSdk: Int?,
    deviceSdk: Int,
    hasSigningCertificate: Boolean,
  ): String? {
    return when {
      packageName == currentPackageName || packageName == RELEASE_APPLICATION_ID -> "self_update"
      split -> "split_package"
      minimumSdk != null && minimumSdk > deviceSdk -> "incompatible_sdk"
      !hasSigningCertificate -> "missing_signature"
      else -> null
    }
  }

  private const val RELEASE_APPLICATION_ID = "org.dlaproject.launcher"
}
