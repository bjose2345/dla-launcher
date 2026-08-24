package org.dlaproject.launcher

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReadOnlyWorkLocatorTest {
  @Test
  fun acceptsOnlyExactReadOnlyWorkLocators() {
    listOf(
      "dla-launcher://works/RJ01326398",
      "DLA-LAUNCHER://WORKS/bj12345",
      "dla-launcher://works/vj1234567890",
    ).forEach { locator ->
      assertTrue("rejected $locator", ReadOnlyWorkLocator.accepts(locator))
    }
  }

  @Test
  fun rejectsAliasesAndWriteCapableRoutesBeforeUrlParsing() {
    listOf(
      null,
      "",
      "https://works/RJ01326398",
      "dla-launcher://scanner/RJ01326398",
      "dla-launcher://import/RJ01326398",
      "dla-launcher://launch/RJ01326398",
      "dla-launcher://works.example/RJ01326398",
      "dla-launcher://user@works/RJ01326398",
      "dla-launcher://works:443/RJ01326398",
      "dla-launcher://works/RJ01326398?launch=true",
      "dla-launcher://works/RJ01326398?",
      "dla-launcher://works/RJ01326398#section",
      "dla-launcher://works/RJ01326398#",
      "dla-launcher://works/RJ01326398/extra",
      "dla-launcher://works/RJ01326398/",
      "dla-launcher://works/%52J01326398",
      "dla-launcher://works/%2e/RJ01326398",
      " dla-launcher://works/RJ01326398",
      "dla-launcher://works/RJ01326398 ",
      "dla-launcher://works/RJ1234",
      "dla-launcher://works/RJ12345678901",
      "dla-launcher://works/RJ12345.exe",
      "dla-launcher://workſ/RJ01326398",
      "dla-launcher://works/🧙12345",
      "🧙dla-launcher://works/RJ01326398",
    ).forEach { locator ->
      assertFalse("accepted $locator", ReadOnlyWorkLocator.accepts(locator))
    }
  }
}
