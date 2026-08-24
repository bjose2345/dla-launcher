package org.dlaproject.launcher.androidpackage

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.provider.Settings
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileOutputStream
import java.security.MessageDigest
import java.util.UUID

@InvokeArg
class AndroidPackageInstallArgs {
  lateinit var selectionId: String
}

@InvokeArg
class AndroidInstalledAppsArgs {
  lateinit var packageNames: Array<String>
}

@InvokeArg
class AndroidAppLaunchArgs {
  lateinit var packageName: String
  lateinit var expectedSigningCertificateSha256: Array<String>
}

@TauriPlugin
class AndroidPackagePlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun readState(invoke: Invoke) {
    invoke.resolve(AndroidPackageStateStore.readState(activity))
  }

  @Command
  fun selectPackage(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = APK_MIME_TYPE
      addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    startActivityForResult(invoke, intent, "packageSelected")
  }

  @ActivityCallback
  fun packageSelected(invoke: Invoke, result: ActivityResult) {
    if (result.resultCode == Activity.RESULT_CANCELED) {
      invoke.resolve(AndroidPackageStateStore.readState(activity))
      return
    }
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      invoke.reject("apk_selection_missing")
      return
    }
    Thread {
      try {
        selectAndInspect(uri)
        invoke.resolve(AndroidPackageStateStore.readState(activity))
      } catch (error: Exception) {
        invoke.reject(adapterFailure("apk_inspection_failed", error))
      }
    }.start()
  }

  @Command
  fun clearSelection(invoke: Invoke) {
    AndroidPackageStateStore.clearSelection(activity)
    invoke.resolve(AndroidPackageStateStore.readState(activity))
  }

  @Command
  fun openSourceApproval(invoke: Invoke) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
      invoke.resolve(AndroidPackageStateStore.readState(activity))
      return
    }
    val intent = Intent(
      Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
      Uri.parse("package:${activity.packageName}"),
    )
    if (intent.resolveActivity(activity.packageManager) == null) {
      invoke.reject("apk_source_settings_unavailable")
      return
    }
    startActivityForResult(invoke, intent, "sourceApprovalReturned")
  }

  @ActivityCallback
  @Suppress("UNUSED_PARAMETER")
  fun sourceApprovalReturned(invoke: Invoke, _result: ActivityResult) {
    invoke.resolve(AndroidPackageStateStore.readState(activity))
  }

  @Command
  fun requestInstall(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(AndroidPackageInstallArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(adapterFailure("apk_install_request_invalid", error))
      return
    }
    Thread {
      val operationId = UUID.randomUUID().toString()
      try {
        requestInstall(operationId, args.selectionId)
      } catch (error: Exception) {
        AndroidPackageStateStore.saveInstallStatus(
          activity,
          operationId,
          args.selectionId,
          "failed",
          technicalDetail = adapterFailure("apk_install_failed", error),
        )
      }
      invoke.resolve(AndroidPackageStateStore.readState(activity))
    }.start()
  }

  @Command
  fun inspectInstalledApps(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(AndroidInstalledAppsArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(adapterFailure("android_app_observation_request_invalid", error))
      return
    }
    if (args.packageNames.size > MAX_ASSOCIATIONS ||
      args.packageNames.distinct().size != args.packageNames.size ||
      args.packageNames.any { !AndroidAppPolicy.validPackageName(it) }) {
      invoke.reject("android_app_observation_request_invalid")
      return
    }
    Thread {
      try {
        val observations = args.packageNames.map { packageName -> observeInstalledApp(packageName) }
        invoke.resolve(
          app.tauri.plugin.JSObject()
            .put("observations", org.json.JSONArray(observations)),
        )
      } catch (error: Exception) {
        invoke.reject(adapterFailure("android_app_observation_failed", error))
      }
    }.start()
  }

  @Command
  fun launchInstalledApp(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(AndroidAppLaunchArgs::class.java)
    } catch (error: Exception) {
      invoke.reject(adapterFailure("android_app_launch_request_invalid", error))
      return
    }
    if (!AndroidAppPolicy.validPackageName(args.packageName) ||
      args.expectedSigningCertificateSha256.isEmpty() ||
      args.expectedSigningCertificateSha256.size > MAX_CERTIFICATES ||
      args.expectedSigningCertificateSha256.any { !AndroidAppPolicy.validSha256(it) }) {
      invoke.reject("android_app_launch_request_invalid")
      return
    }
    val observation = observeInstalledApp(args.packageName)
    if (observation.optString("state") != "installed") {
      invoke.reject("android_app_not_installed")
      return
    }
    val observedCertificates = observation.getJSONArray("signingCertificateSha256")
    val signerMatches = AndroidAppPolicy.signerMatches(
      args.expectedSigningCertificateSha256,
      (0 until observedCertificates.length()).map(observedCertificates::getString),
    )
    if (!signerMatches) {
      invoke.reject("android_app_signer_mismatch")
      return
    }
    if (!observation.optBoolean("launchable")) {
      invoke.reject("android_app_not_launchable")
      return
    }
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        activity.packageManager
          .getLaunchIntentSenderForPackage(args.packageName)
          .sendIntent(activity, 0, null, null, null)
      } else {
        val intent = activity.packageManager.getLaunchIntentForPackage(args.packageName)
          ?: throw IllegalStateException("installed package has no launcher activity")
        activity.startActivity(intent)
      }
      invoke.resolve(app.tauri.plugin.JSObject().put("packageName", args.packageName))
    } catch (error: Exception) {
      invoke.reject(adapterFailure("android_app_launch_failed", error))
    }
  }

  private fun selectAndInspect(uri: Uri) {
    val displayName = displayName(uri)
    if (!displayName.endsWith(".apk", ignoreCase = true)) {
      throw IllegalArgumentException("selected document is not an APK")
    }

    val selectionId = UUID.randomUUID().toString()
    val directory = AndroidPackageStateStore.stagingDirectory(activity)
    val partial = File(directory, ".$selectionId.partial")
    partial.delete()
    try {
      val digest = MessageDigest.getInstance("SHA-256")
      val available = (directory.usableSpace - AndroidPackageStateStore.STAGING_RESERVE_BYTES)
        .coerceAtLeast(0L)
      val declaredSize = activity.contentResolver
        .openFileDescriptor(uri, "r")
        ?.use { it.statSize }
        ?: -1L
      if (declaredSize > available) {
        throw IllegalStateException("not enough private storage for the selected APK")
      }

      var copied = 0L
      activity.contentResolver.openInputStream(uri)?.use { input ->
        FileOutputStream(partial).use { output ->
          val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
          while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            copied += count
            if (copied > available) {
              throw IllegalStateException("selected APK exceeded available private storage")
            }
            output.write(buffer, 0, count)
            digest.update(buffer, 0, count)
          }
          output.fd.sync()
        }
      } ?: throw IllegalArgumentException("selected document could not be opened")
      if (copied == 0L) throw IllegalArgumentException("selected APK is empty")

      val archive = parseArchive(partial)
      val sha256 = digest.digest().toHex()
      val blockReason = AndroidPackagePolicy.blockReason(
        packageName = archive.packageInfo.packageName,
        currentPackageName = activity.packageName,
        split = archive.split,
        minimumSdk = archive.minimumSdk,
        deviceSdk = Build.VERSION.SDK_INT,
        hasSigningCertificate = archive.signingCertificates.isNotEmpty(),
      )
      val installable = blockReason == null
      val inspection = AndroidPackageStateStore.inspection(
        selectionId = selectionId,
        displayName = displayName,
        applicationLabel = archive.applicationLabel,
        packageName = archive.packageInfo.packageName,
        versionName = archive.packageInfo.versionName,
        versionCode = archive.versionCode,
        sizeBytes = copied,
        sha256 = sha256,
        minimumSdk = archive.minimumSdk,
        targetSdk = archive.targetSdk,
        signingCertificates = archive.signingCertificates,
        installable = installable,
        blockReason = blockReason,
      )

      AndroidPackageStateStore.clearSelection(activity)
      if (installable) {
        val staged = AndroidPackageStateStore.stagedFile(activity, selectionId)
        if (!partial.renameTo(staged)) {
          throw IllegalStateException("selected APK could not be activated in private storage")
        }
      } else {
        partial.delete()
      }
      AndroidPackageStateStore.saveInspection(activity, inspection)
    } finally {
      partial.delete()
    }
  }

  private fun requestInstall(operationId: String, selectionId: String) {
    val inspection = AndroidPackageStateStore.readInspection(activity)
      ?: throw IllegalStateException("no APK has been inspected")
    if (inspection.optString("selectionId") != selectionId ||
      !inspection.optBoolean("installable")) {
      throw IllegalArgumentException("APK selection is not installable")
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
      !activity.packageManager.canRequestPackageInstalls()) {
      AndroidPackageStateStore.saveInstallStatus(
        activity,
        operationId,
        selectionId,
        "approval_required",
      )
      return
    }

    val staged = AndroidPackageStateStore.stagedFile(activity, selectionId)
    if (!staged.isFile) throw IllegalStateException("private APK copy is missing")
    val expectedSha256 = inspection.getString("sha256")
    if (!sha256(staged).equals(expectedSha256, ignoreCase = true)) {
      throw IllegalStateException("private APK copy changed after inspection")
    }
    val archive = parseArchive(staged)
    val blockReason = AndroidPackagePolicy.blockReason(
      packageName = archive.packageInfo.packageName,
      currentPackageName = activity.packageName,
      split = archive.split,
      minimumSdk = archive.minimumSdk,
      deviceSdk = Build.VERSION.SDK_INT,
      hasSigningCertificate = archive.signingCertificates.isNotEmpty(),
    )
    if (archive.packageInfo.packageName != inspection.getString("packageName") ||
      blockReason != null) {
      throw IllegalStateException("APK identity changed after inspection")
    }

    AndroidPackageStateStore.saveInstallStatus(
      activity,
      operationId,
      selectionId,
      "preparing",
    )
    val installer = activity.packageManager.packageInstaller
    val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
      .apply {
        setAppPackageName(archive.packageInfo.packageName)
        setSize(staged.length())
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
          setInstallReason(PackageManager.INSTALL_REASON_USER)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
          setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
          setPackageSource(PackageInstaller.PACKAGE_SOURCE_LOCAL_FILE)
        }
      }
    var sessionId: Int? = null
    try {
      sessionId = installer.createSession(params)
      AndroidPackageStateStore.saveInstallStatus(
        activity,
        operationId,
        selectionId,
        "preparing",
        sessionId = sessionId,
      )
      installer.openSession(sessionId).use { session ->
        session.openWrite("base.apk", 0, staged.length()).use { output ->
          staged.inputStream().use { input -> input.copyTo(output) }
          session.fsync(output)
        }
        val callback = Intent(activity, PackageInstallResultReceiver::class.java).apply {
          action = "${activity.packageName}.APK_INSTALL_RESULT"
          data = Uri.parse("dla-apk-install://$operationId")
          putExtra(AndroidPackageStateStore.EXTRA_OPERATION_ID, operationId)
          putExtra(AndroidPackageStateStore.EXTRA_SELECTION_ID, selectionId)
        }
        val pending = PendingIntent.getBroadcast(
          activity,
          operationId.hashCode(),
          callback,
          PendingIntent.FLAG_UPDATE_CURRENT or if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            PendingIntent.FLAG_MUTABLE
          } else {
            0
          },
        )
        session.commit(pending.intentSender)
      }
    } catch (error: Exception) {
      sessionId?.let { runCatching { installer.abandonSession(it) } }
      throw error
    }
  }

  private fun parseArchive(file: File): ParsedArchive {
    val signatureFlags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      PackageManager.GET_SIGNING_CERTIFICATES
    } else {
      @Suppress("DEPRECATION")
      PackageManager.GET_SIGNATURES
    }
    val info = activity.packageManager.getPackageArchiveInfo(file.absolutePath, signatureFlags)
      ?: throw IllegalArgumentException("Android could not parse the selected APK")
    val application = info.applicationInfo
      ?: throw IllegalArgumentException("APK has no application metadata")
    application.sourceDir = file.absolutePath
    application.publicSourceDir = file.absolutePath
    val label = activity.packageManager.getApplicationLabel(application).toString()
      .take(MAX_IDENTITY_LENGTH)
      .ifBlank { info.packageName }
    val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      info.signingInfo?.apkContentsSigners?.toList().orEmpty()
    } else {
      @Suppress("DEPRECATION")
      info.signatures?.toList().orEmpty()
    }
    val split = !info.splitNames.isNullOrEmpty() || !application.splitSourceDirs.isNullOrEmpty()
    return ParsedArchive(
      packageInfo = info,
      applicationLabel = label,
      versionCode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        info.longVersionCode
      } else {
        @Suppress("DEPRECATION")
        info.versionCode.toLong()
      },
      minimumSdk = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
        application.minSdkVersion
      } else {
        null
      },
      targetSdk = application.targetSdkVersion.takeIf { it > 0 },
      signingCertificates = signatures
        .map { signature -> MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).toHex() }
        .distinct()
        .sorted(),
      split = split,
    )
  }

  private fun observeInstalledApp(packageName: String): org.json.JSONObject {
    val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      PackageManager.GET_SIGNING_CERTIFICATES
    } else {
      @Suppress("DEPRECATION")
      PackageManager.GET_SIGNATURES
    }
    val info = try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        activity.packageManager.getPackageInfo(
          packageName,
          PackageManager.PackageInfoFlags.of(flags.toLong()),
        )
      } else {
        @Suppress("DEPRECATION")
        activity.packageManager.getPackageInfo(packageName, flags)
      }
    } catch (_: PackageManager.NameNotFoundException) {
      return org.json.JSONObject()
        .put("packageName", packageName)
        .put("state", "missing")
        .put("signingCertificateSha256", org.json.JSONArray())
        .put("launchable", false)
    } catch (error: Exception) {
      return unavailableObservation(packageName, error)
    }
    return try {
      val application = info.applicationInfo
        ?: throw IllegalStateException("installed package has no application metadata")
      val label = activity.packageManager.getApplicationLabel(application).toString()
        .take(MAX_IDENTITY_LENGTH)
        .ifBlank { packageName }
      org.json.JSONObject()
        .put("packageName", packageName)
        .put("state", "installed")
        .put("applicationLabel", label)
        .put("versionName", info.versionName)
        .put(
          "versionCode",
          if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.longVersionCode.toString()
          } else {
            @Suppress("DEPRECATION")
            info.versionCode.toString()
          },
        )
        .put(
          "signingCertificateSha256",
          org.json.JSONArray(installedSigningCertificates(info)),
        )
        .put(
          "launchable",
          activity.packageManager.getLaunchIntentForPackage(packageName) != null,
        )
    } catch (error: Exception) {
      unavailableObservation(packageName, error)
    }
  }

  private fun unavailableObservation(
    packageName: String,
    error: Exception,
  ): org.json.JSONObject = org.json.JSONObject()
    .put("packageName", packageName)
    .put("state", "unavailable")
    .put("signingCertificateSha256", org.json.JSONArray())
    .put("launchable", false)
    .put("technicalDetail", adapterFailure("android_app_observation_failed", error))

  private fun installedSigningCertificates(info: PackageInfo): List<String> {
    val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      val signingInfo = info.signingInfo
      if (signingInfo?.hasMultipleSigners() == true) {
        signingInfo.apkContentsSigners?.toList().orEmpty()
      } else {
        signingInfo?.signingCertificateHistory?.toList()
          ?: signingInfo?.apkContentsSigners?.toList().orEmpty()
      }
    } else {
      @Suppress("DEPRECATION")
      info.signatures?.toList().orEmpty()
    }
    return signatures
      .map { signature ->
        MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).toHex()
      }
      .distinct()
      .sorted()
  }

  private fun displayName(uri: Uri): String {
    activity.contentResolver.query(
      uri,
      arrayOf(OpenableColumns.DISPLAY_NAME),
      null,
      null,
      null,
    )?.use { cursor ->
      if (cursor.moveToFirst()) {
        val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
        if (index >= 0) {
          return cursor.getString(index).orEmpty().take(MAX_IDENTITY_LENGTH)
        }
      }
    }
    throw IllegalArgumentException("selected document has no display name")
  }

  private fun sha256(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().use { input ->
      val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
      while (true) {
        val count = input.read(buffer)
        if (count < 0) break
        digest.update(buffer, 0, count)
      }
    }
    return digest.digest().toHex()
  }

  private fun ByteArray.toHex(): String =
    joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }

  private fun adapterFailure(code: String, error: Exception): String {
    val detail = error.message?.replace(Regex("[\\r\\n]+"), " ")?.take(MAX_ERROR_LENGTH)
    return if (detail.isNullOrBlank()) code else "$code: $detail"
  }

  private data class ParsedArchive(
    val packageInfo: PackageInfo,
    val applicationLabel: String,
    val versionCode: Long,
    val minimumSdk: Int?,
    val targetSdk: Int?,
    val signingCertificates: List<String>,
    val split: Boolean,
  )

  private companion object {
    const val APK_MIME_TYPE = "application/vnd.android.package-archive"
    const val MAX_IDENTITY_LENGTH = 512
    const val MAX_ERROR_LENGTH = 512
    const val MAX_ASSOCIATIONS = 512
    const val MAX_CERTIFICATES = 32
  }
}
