package org.dlaproject.launcher.androidpackage

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidAppPolicyTest {
  @Test
  fun acceptsOnlyBoundedAsciiPackageNames() {
    assertTrue(AndroidAppPolicy.validPackageName("org.dlaproject.fixture_2"))
    assertFalse(AndroidAppPolicy.validPackageName("fixture"))
    assertFalse(AndroidAppPolicy.validPackageName("org.dlaproject../fixture"))
    assertFalse(AndroidAppPolicy.validPackageName("org.dlaproject.ápp"))
  }

  @Test
  fun signerComparison_isCaseInsensitiveButRequiresAnExactFingerprint() {
    val reviewed = arrayOf("a".repeat(64))
    assertTrue(AndroidAppPolicy.signerMatches(reviewed, listOf("A".repeat(64))))
    assertFalse(AndroidAppPolicy.signerMatches(reviewed, listOf("b".repeat(64))))
    assertFalse(AndroidAppPolicy.signerMatches(emptyArray(), listOf("a".repeat(64))))
  }

  @Test
  fun validatesSha256Shape() {
    assertTrue(AndroidAppPolicy.validSha256("0a".repeat(32)))
    assertFalse(AndroidAppPolicy.validSha256("g".repeat(64)))
    assertFalse(AndroidAppPolicy.validSha256("a".repeat(63)))
  }
}
