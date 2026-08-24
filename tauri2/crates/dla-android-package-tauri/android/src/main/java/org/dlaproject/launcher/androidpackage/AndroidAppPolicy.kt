package org.dlaproject.launcher.androidpackage

internal object AndroidAppPolicy {
  fun validPackageName(value: String): Boolean {
    val segments = value.split('.')
    return value.length <= 255 && segments.size >= 2 && segments.all { segment ->
      segment.isNotEmpty() && segment.first().isAsciiLetter() &&
        segment.all { character -> character.isAsciiLetterOrDigit() || character == '_' }
    }
  }

  fun validSha256(value: String): Boolean =
    value.length == 64 && value.all { character ->
      character in '0'..'9' || character.lowercaseChar() in 'a'..'f'
    }

  fun signerMatches(expected: Array<String>, observed: List<String>): Boolean =
    expected.any { expectedFingerprint ->
      observed.any { observedFingerprint ->
        expectedFingerprint.equals(observedFingerprint, ignoreCase = true)
      }
    }

  private fun Char.isAsciiLetter(): Boolean = this in 'a'..'z' || this in 'A'..'Z'

  private fun Char.isAsciiLetterOrDigit(): Boolean = isAsciiLetter() || this in '0'..'9'
}
