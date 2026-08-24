package org.dlaproject.launcher

import android.content.Intent
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    rejectUnsupportedReadOnlyWorkIntent(intent)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onNewIntent(intent: Intent) {
    rejectUnsupportedReadOnlyWorkIntent(intent)
    super.onNewIntent(intent)
  }

  private fun rejectUnsupportedReadOnlyWorkIntent(intent: Intent) {
    if (isViewIntent(intent.action) && !ReadOnlyWorkLocator.accepts(intent.dataString)) {
      intent.data = null
    }
  }

  private fun isViewIntent(action: String?): Boolean {
    return action == Intent.ACTION_VIEW || action == "org.chromium.arc.intent.action.VIEW"
  }
}
