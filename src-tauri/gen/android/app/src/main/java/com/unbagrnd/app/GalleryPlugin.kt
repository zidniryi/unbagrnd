// Publishes exported images into the shared MediaStore ("Pictures/unbagrnd")
// so they show up in Gallery/Photos right away, and opens the most recently
// published image in the user's photo viewer as the Android substitute for
// "reveal in folder" (unsupported on this platform by tauri-plugin-opener).
//
// Registered from Rust in `android_gallery.rs` via `register_android_plugin`,
// which loads this class by its fully-qualified name through the activity's
// classloader - no separate Gradle module is needed for that to work, this
// file just needs to be on the app module's classpath, which it already is.

package com.unbagrnd.app

import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.IOException

@InvokeArg
class SaveToGalleryArgs {
  lateinit var bytesBase64: String
  lateinit var displayName: String
  lateinit var mimeType: String
}

@InvokeArg
class OpenInGalleryArgs {
  lateinit var uri: String
}

@TauriPlugin
class GalleryPlugin(private val activity: Activity) : Plugin(activity) {

  @Command
  fun saveToGallery(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(SaveToGalleryArgs::class.java)
    } catch (ex: Exception) {
      invoke.reject(ex.message ?: "invalid arguments")
      return
    }

    val resolver = activity.contentResolver
    val values = ContentValues().apply {
      put(MediaStore.Images.Media.DISPLAY_NAME, args.displayName)
      put(MediaStore.Images.Media.MIME_TYPE, args.mimeType)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/unbagrnd")
        put(MediaStore.Images.Media.IS_PENDING, 1)
      }
    }

    val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
    if (uri == null) {
      invoke.reject("could not create a gallery entry")
      return
    }

    try {
      val bytes = Base64.decode(args.bytesBase64, Base64.DEFAULT)
      val stream = resolver.openOutputStream(uri)
        ?: throw IOException("could not open the gallery entry for writing")
      stream.use { it.write(bytes) }

      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        values.clear()
        values.put(MediaStore.Images.Media.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
      }

      val ret = JSObject()
      ret.put("uri", uri.toString())
      invoke.resolve(ret)
    } catch (ex: Exception) {
      resolver.delete(uri, null, null)
      invoke.reject(ex.message ?: "could not save to the gallery")
    }
  }

  @Command
  fun openInGallery(invoke: Invoke) {
    val args = try {
      invoke.parseArgs(OpenInGalleryArgs::class.java)
    } catch (ex: Exception) {
      invoke.reject(ex.message ?: "invalid arguments")
      return
    }

    try {
      val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(Uri.parse(args.uri), "image/*")
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      activity.startActivity(intent)
      invoke.resolve(JSObject())
    } catch (ex: Exception) {
      invoke.reject(ex.message ?: "could not open the gallery")
    }
  }
}
