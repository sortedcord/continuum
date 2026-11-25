package io.github.sortedcord.continuum.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import io.github.sortedcord.continuum.ContinuumApp
import io.github.sortedcord.continuum.R
import io.github.sortedcord.continuum.ffi.AdvertisementFfi
import io.github.sortedcord.continuum.platform.BleAdapter
import io.github.sortedcord.continuum.platform.NsdAdapter
import io.github.sortedcord.continuum.ui.MainActivity

/**
 * Keeps the node running in the background and connects it to the radios:
 * advertisements from Rust go out over BLE and mDNS, and every beacon or
 * mDNS record we see goes back into Rust.
 */
class ContinuumService : Service() {
    private val repository by lazy { (application as ContinuumApp).repository }
    private lateinit var ble: BleAdapter
    private lateinit var nsd: NsdAdapter
    private var serviceType: String? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        startForeground(
            NOTIFICATION_ID,
            notification(),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )
        val node = repository.continuum
        ble = BleAdapter(this, repository::log)
        nsd = NsdAdapter(
            this,
            repository::log,
            onResolved = { name, txt, host, port ->
                runCatching { node.observeLan(name, txt, host.hostAddress ?: return@runCatching, port.toUShort()) }
            },
            onLost = { name -> node.withdrawLan(name) },
        )
        repository.onAdvertise = ::advertise
        runCatching { node.start(repository.listener) }
            .onSuccess { port ->
                repository.setRunning(true)
                repository.log("Listening on port $port")
            }
            .onFailure { repository.log("Could not start: ${it.message}") }
    }

    private fun advertise(ad: AdvertisementFfi) {
        val node = repository.continuum
        nsd.register(ad.serviceName, ad.serviceType, ad.port.toInt(), ad.txt)
        if (serviceType != ad.serviceType) {
            serviceType = ad.serviceType
            nsd.startDiscovery(ad.serviceType)
        }
        if (ble.available) {
            ble.advertise(ad.bleServiceUuid, ad.beacon)
            ble.startScan(ad.bleServiceUuid) { data, rssi -> node.observeBle(data, rssi.toShort()) }
        } else {
            repository.log("Bluetooth is off; using Wi-Fi only")
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onDestroy() {
        repository.onAdvertise = null
        ble.stopScan()
        ble.stopAdvertising()
        nsd.stopDiscovery()
        nsd.unregister()
        repository.continuum.stop()
        repository.setRunning(false)
        super.onDestroy()
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.channel_presence), NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, ContinuumService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.notification_running))
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val CHANNEL = "presence"
        private const val NOTIFICATION_ID = 1
        private const val ACTION_STOP = "stop"

        fun start(context: Context) {
            context.startForegroundService(Intent(context, ContinuumService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, ContinuumService::class.java))
        }
    }
}
