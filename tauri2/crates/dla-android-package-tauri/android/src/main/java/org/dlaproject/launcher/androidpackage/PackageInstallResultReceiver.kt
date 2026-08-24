package org.dlaproject.launcher.androidpackage

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build

class PackageInstallResultReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val operationId = intent.getStringExtra(AndroidPackageStateStore.EXTRA_OPERATION_ID)
      ?: return
    val selectionId = intent.getStringExtra(AndroidPackageStateStore.EXTRA_SELECTION_ID)
      ?: return
    val sessionId = intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, -1)
      .takeIf { it >= 0 }
    when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
      PackageInstaller.STATUS_PENDING_USER_ACTION -> {
        AndroidPackageStateStore.saveInstallStatus(
          context,
          operationId,
          selectionId,
          "awaiting_user_confirmation",
          sessionId = sessionId,
        )
        val confirmation = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
          intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
        } else {
          @Suppress("DEPRECATION")
          intent.getParcelableExtra(Intent.EXTRA_INTENT)
        }
        if (confirmation == null) {
          AndroidPackageStateStore.saveInstallStatus(
            context,
            operationId,
            selectionId,
            "failed",
            technicalDetail = "Android did not provide an installation confirmation intent",
            sessionId = sessionId,
          )
          return
        }
        try {
          confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          context.startActivity(confirmation)
        } catch (error: Exception) {
          AndroidPackageStateStore.saveInstallStatus(
            context,
            operationId,
            selectionId,
            "failed",
            technicalDetail = error.message ?: "Android could not open the installation confirmation",
            sessionId = sessionId,
          )
        }
      }

      PackageInstaller.STATUS_SUCCESS -> {
        AndroidPackageStateStore.saveInstallStatus(
          context,
          operationId,
          selectionId,
          "installed",
          sessionId = sessionId,
        )
        AndroidPackageStateStore.deleteStagedFile(context, selectionId)
      }

      PackageInstaller.STATUS_FAILURE_ABORTED -> AndroidPackageStateStore.saveInstallStatus(
        context,
        operationId,
        selectionId,
        "cancelled",
        sessionId = sessionId,
      )

      else -> AndroidPackageStateStore.saveInstallStatus(
        context,
        operationId,
        selectionId,
        "failed",
        technicalDetail = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE),
        sessionId = sessionId,
      )
    }
  }
}
