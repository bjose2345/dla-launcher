package org.dlaproject.launcher

object ReadOnlyWorkLocator {
  private const val routePrefix = "dla-launcher://works/"

  fun accepts(value: String?): Boolean {
    if (value == null || value.length < routePrefix.length) {
      return false
    }

    if (!matchesAsciiIgnoreCase(value, 0, routePrefix)) {
      return false
    }

    val code = value.substring(routePrefix.length)
    if (code.length !in 7..12) {
      return false
    }

    val prefix = code.substring(0, 2)
    if (!matchesAsciiIgnoreCase(prefix, 0, "RJ") &&
      !matchesAsciiIgnoreCase(prefix, 0, "BJ") &&
      !matchesAsciiIgnoreCase(prefix, 0, "VJ")
    ) {
      return false
    }

    val digits = code.substring(2)
    return digits.length in 5..10 && digits.all { it in '0'..'9' }
  }

  private fun matchesAsciiIgnoreCase(value: String, offset: Int, expected: String): Boolean {
    if (offset < 0 || value.length - offset < expected.length) {
      return false
    }

    return expected.indices.all { index ->
      asciiLower(value[offset + index]) == asciiLower(expected[index])
    }
  }

  private fun asciiLower(value: Char): Char {
    return if (value in 'A'..'Z') (value.code + ('a'.code - 'A'.code)).toChar() else value
  }
}
