package org.dlaproject.launcher.androidpackage

import android.content.Context
import android.os.Build
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

internal object AndroidPackageStateStore {
  const val EXTRA_OPERATION_ID = "org.dlaproject.launcher.extra.APK_OPERATION_ID"
  const val EXTRA_SELECTION_ID = "org.dlaproject.launcher.extra.APK_SELECTION_ID"
  const val STAGING_RESERVE_BYTES = 128L * 1024L * 1024L

  private const val preferencesName = "dla_android_package_state"
  private const val inspectionKey = "inspection"
  private const val installStatusKey = "install_status"
  private const val stagingDirectoryName = "android-package-staging"
  private const val stagedLifetimeMillis = 24L * 60L * 60L * 1000L
  private const val missingSessionGraceMillis = 30_000L

  fun readState(context: Context): JSObject {
    cleanupExpired(context)
    reconcileInstallSession(context)
    return JSObject()
      .put("capability", capability(context))
      .put("inspection", readInspection(context))
      .put("installStatus", readInstallStatus(context))
  }

  fun capability(context: Context): JSObject {
    val approved = Build.VERSION.SDK_INT < Build.VERSION_CODES.O ||
      context.packageManager.canRequestPackageInstalls()
    return JSObject()
      .put("status", if (approved) "ready" else "approval_required")
      .put("deviceSdk", Build.VERSION.SDK_INT)
  }

  fun stagingDirectory(context: Context): File {
    return File(context.noBackupFilesDir, stagingDirectoryName).apply { mkdirs() }
  }

  fun stagedFile(context: Context, selectionId: String): File {
    return File(stagingDirectory(context), "$selectionId.apk")
  }

  fun readInspection(context: Context): JSONObject? {
    return readObject(context, inspectionKey)
  }

  fun saveInspection(context: Context, inspection: JSONObject) {
    preferences(context).edit()
      .putString(inspectionKey, inspection.toString())
      .remove(installStatusKey)
      .apply()
  }

  fun readInstallStatus(context: Context): JSONObject? {
    return readObject(context, installStatusKey)
  }

  fun saveInstallStatus(
    context: Context,
    operationId: String,
    selectionId: String,
    state: String,
    technicalDetail: String? = null,
    sessionId: Int? = null,
  ) {
    val previousSessionId = readInstallStatus(context)
      ?.takeIf {
        it.optString("operationId") == operationId &&
          it.optString("selectionId") == selectionId
      }
      ?.optInt("sessionId", -1)
      ?.takeIf { it >= 0 }
    val status = JSONObject()
      .put("operationId", operationId)
      .put("selectionId", selectionId)
      .put("state", state)
      .put("updatedAtEpochMillis", System.currentTimeMillis())
    (sessionId ?: previousSessionId)?.let { status.put("sessionId", it) }
    if (!technicalDetail.isNullOrBlank()) {
      status.put("technicalDetail", technicalDetail.take(2048))
    }
    preferences(context).edit().putString(installStatusKey, status.toString()).apply()
  }

  fun clearSelection(context: Context) {
    readInspection(context)?.optString("selectionId")
      ?.takeIf { it.isNotBlank() }
      ?.let { stagedFile(context, it).delete() }
    preferences(context).edit().remove(inspectionKey).remove(installStatusKey).apply()
  }

  fun deleteStagedFile(context: Context, selectionId: String) {
    stagedFile(context, selectionId).delete()
  }

  fun inspection(
    selectionId: String,
    displayName: String,
    applicationLabel: String,
    packageName: String,
    versionName: String?,
    versionCode: Long,
    sizeBytes: Long,
    sha256: String,
    minimumSdk: Int?,
    targetSdk: Int?,
    signingCertificates: List<String>,
    installable: Boolean,
    blockReason: String?,
  ): JSONObject {
    val value = JSONObject()
      .put("selectionId", selectionId)
      .put("displayName", displayName)
      .put("applicationLabel", applicationLabel)
      .put("packageName", packageName)
      .put("versionCode", versionCode.toString())
      .put("sizeBytes", sizeBytes)
      .put("sha256", sha256)
      .put("signingCertificateSha256", JSONArray(signingCertificates))
      .put("installable", installable)
    if (versionName != null) value.put("versionName", versionName)
    if (minimumSdk != null) value.put("minimumSdk", minimumSdk)
    if (targetSdk != null) value.put("targetSdk", targetSdk)
    if (blockReason != null) value.put("blockReason", blockReason)
    return value
  }

  private fun preferences(context: Context) =
    context.getSharedPreferences(preferencesName, Context.MODE_PRIVATE)

  private fun readObject(context: Context, key: String): JSONObject? {
    val serialized = preferences(context).getString(key, null) ?: return null
    return try {
      JSONObject(serialized)
    } catch (_: Exception) {
      preferences(context).edit().remove(key).apply()
      null
    }
  }

  private fun cleanupExpired(context: Context) {
    val directory = stagingDirectory(context)
    val cutoff = System.currentTimeMillis() - stagedLifetimeMillis
    directory.listFiles()?.forEach { file ->
      if (!file.isFile || file.lastModified() < cutoff) file.delete()
    }
    val inspection = readInspection(context) ?: return
    val selectionId = inspection.optString("selectionId")
    val status = readInstallStatus(context)
      ?.takeIf { it.optString("selectionId") == selectionId }
      ?.optString("state")
    val retainsInspection = status == "preparing" ||
      status == "awaiting_user_confirmation" ||
      status == "installed"
    if (inspection.optBoolean("installable") &&
      !retainsInspection &&
      !stagedFile(context, selectionId).isFile) {
      preferences(context).edit().remove(inspectionKey).remove(installStatusKey).apply()
    }
  }

  private fun reconcileInstallSession(context: Context) {
    val status = readInstallStatus(context) ?: return
    val state = status.optString("state")
    if (state != "preparing" && state != "awaiting_user_confirmation") return
    val updatedAt = status.optLong("updatedAtEpochMillis", 0L)
    if (System.currentTimeMillis() - updatedAt < missingSessionGraceMillis) return
    val sessionId = status.optInt("sessionId", -1)
    val activeSessions = runCatching {
      context.packageManager.packageInstaller.mySessions.map { it.sessionId }
    }.getOrNull() ?: return
    if (sessionId >= 0 && sessionId in activeSessions) return
    saveInstallStatus(
      context,
      status.optString("operationId"),
      status.optString("selectionId"),
      "failed",
      technicalDetail = "Android installation session is no longer active",
      sessionId = sessionId.takeIf { it >= 0 },
    )
  }
}
