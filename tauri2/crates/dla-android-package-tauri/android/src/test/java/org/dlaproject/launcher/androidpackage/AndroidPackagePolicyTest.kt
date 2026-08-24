package org.dlaproject.launcher.androidpackage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AndroidPackagePolicyTest {
  @Test
  fun acceptsOneSignedStandalonePackageForTheCurrentDevice() {
    assertNull(blockReason())
  }

  @Test
  fun refusesTheDebugAndReleaseLauncherPackages() {
    assertEquals(
      "self_update",
      blockReason(packageName = "org.dlaproject.launcher.debug"),
    )
    assertEquals(
      "self_update",
      blockReason(packageName = "org.dlaproject.launcher"),
    )
  }

  @Test
  fun refusesSplitIncompatibleAndUnsignedPackages() {
    assertEquals("split_package", blockReason(split = true))
    assertEquals("incompatible_sdk", blockReason(minimumSdk = 37))
    assertEquals("missing_signature", blockReason(hasSigningCertificate = false))
  }

  @Test
  fun appliesTheMostSafetyCriticalReasonFirst() {
    assertEquals(
      "self_update",
      blockReason(
        packageName = "org.dlaproject.launcher.debug",
        split = true,
        minimumSdk = 37,
        hasSigningCertificate = false,
      ),
    )
  }

  private fun blockReason(
    packageName: String = "org.dlaproject.fixture",
    split: Boolean = false,
    minimumSdk: Int? = 24,
    hasSigningCertificate: Boolean = true,
  ): String? {
    return AndroidPackagePolicy.blockReason(
      packageName = packageName,
      currentPackageName = "org.dlaproject.launcher.debug",
      split = split,
      minimumSdk = minimumSdk,
      deviceSdk = 36,
      hasSigningCertificate = hasSigningCertificate,
    )
  }
}
