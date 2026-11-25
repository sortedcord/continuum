package io.github.sortedcord.continuum.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.bluetooth.BluetoothAdapter
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.os.IBinder
import io.github.sortedcord.continuum.ContinuumApp
import io.github.sortedcord.continuum.R
import io.github.sortedcord.continuum.BuildConfig
import io.github.sortedcord.continuum.ffi.AdvertisementFfi
import io.github.sortedcord.continuum.ffi.NodeEventFfi
import java.io.File
import java.net.Inet6Address
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
    private lateinit var ringer: PageRinger
    private var serviceType: String? = null
    @Volatile private var lastAdvertisement: AdvertisementFfi? = null

    /** Starts or stops BLE when the user toggles Bluetooth. */
    private val bluetoothState = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_ON -> lastAdvertisement?.let {
                    repository.log("Bluetooth on")
                    startBle(it)
                }
                BluetoothAdapter.STATE_TURNING_OFF -> {
                    ble.stopScan()
                    ble.stopAdvertising()
                    repository.log("Bluetooth off; using Wi-Fi only")
                }
            }
        }
    }

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
                android.util.Log.d("Continuum", "mDNS resolved $name at ${host.hostAddress}:$port")
                val scope = (host as? Inet6Address)?.scopeId ?: 0
                runCatching {
                    node.observeLan(name, txt, host.hostAddress ?: return@runCatching, port.toUShort(), scope.toUInt())
                }
            },
            onLost = { name ->
                android.util.Log.d("Continuum", "mDNS lost $name")
                node.withdrawLan(name)
            },
        )
        ringer = PageRinger(this)
        repository.onAdvertise = ::advertise
        repository.onPage = { event ->
            when (event) {
                is NodeEventFfi.PageStart -> {
                    repository.log("${event.name} is looking for this phone")
                    ringer.start(event.peerId, event.name, event.durationS.toInt())
                }
                is NodeEventFfi.PageStop -> ringer.stop()
                else -> {}
            }
        }
        registerReceiver(bluetoothState, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED))
        // Debug builds can rotate identifiers every minute for testing:
        //   adb shell run-as io.github.sortedcord.continuum touch files/fast-rotation
        val rotation = if (BuildConfig.DEBUG && File(filesDir, "fast-rotation").exists()) 60u else null
        runCatching { node.start(repository.listener, rotation) }
            .onSuccess { port ->
                repository.setRunning(true)
                repository.log("Listening on port $port")
            }
            .onFailure { repository.log("Could not start: ${it.message}") }
    }

    private fun advertise(ad: AdvertisementFfi) {
        lastAdvertisement = ad
        nsd.register(ad.serviceName, ad.serviceType, ad.port.toInt(), ad.txt)
        if (serviceType != ad.serviceType) {
            serviceType = ad.serviceType
            nsd.startDiscovery(ad.serviceType)
        }
        if (ble.available) {
            startBle(ad)
        } else {
            repository.log("Bluetooth is off; using Wi-Fi only")
        }
    }

    private fun startBle(ad: AdvertisementFfi) {
        val node = repository.continuum
        ble.advertise(ad.bleServiceUuid, ad.beacon)
        ble.startScan(ad.bleServiceUuid) { data, rssi -> node.observeBle(data, rssi.toShort()) }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                repository.enabled = false
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_FOUND -> {
                val peer = ringer.found() ?: intent.getStringExtra(EXTRA_PEER)
                peer?.let { id -> Thread { runCatching { repository.continuum.pageFound(id) } }.start() }
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        repository.onAdvertise = null
        repository.onPage = null
        ringer.stop()
        unregisterReceiver(bluetoothState)
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
        const val ACTION_FOUND = "found"
        const val EXTRA_PEER = "peer"

        fun start(context: Context) {
            (context.applicationContext as ContinuumApp).repository.enabled = true
            context.startForegroundService(Intent(context, ContinuumService::class.java))
        }

        fun stop(context: Context) {
            (context.applicationContext as ContinuumApp).repository.enabled = false
            context.stopService(Intent(context, ContinuumService::class.java))
        }
    }
}
