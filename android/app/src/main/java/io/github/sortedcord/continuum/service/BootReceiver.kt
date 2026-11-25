package io.github.sortedcord.continuum.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import io.github.sortedcord.continuum.ContinuumApp

/** Restarts the presence service after boot or an app update. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val wanted = intent.action == Intent.ACTION_BOOT_COMPLETED ||
            intent.action == Intent.ACTION_MY_PACKAGE_REPLACED
        val repository = (context.applicationContext as ContinuumApp).repository
        if (wanted && repository.enabled) ContinuumService.start(context)
    }
}
