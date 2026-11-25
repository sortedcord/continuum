package io.github.sortedcord.continuum.platform

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import java.net.InetAddress
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

/**
 * Publishes and browses DNS-SD services through NsdManager.
 *
 * On Android 14+ each found service gets a persistent info callback, which
 * reports every address and later updates. Older versions fall back to
 * one-shot resolves, serialised because NsdManager allows only one at a time.
 */
class NsdAdapter(
    context: Context,
    private val log: (String) -> Unit,
    private val onResolved: (name: String, txt: Map<String, String>, host: InetAddress, port: Int) -> Unit,
    private val onLost: (name: String) -> Unit,
) {
    private val nsd = context.getSystemService(NsdManager::class.java)
    private val executor = Executors.newSingleThreadExecutor()
    private var registration: NsdManager.RegistrationListener? = null
    private var discovery: NsdManager.DiscoveryListener? = null
    private val infoCallbacks = ConcurrentHashMap<String, NsdManager.ServiceInfoCallback>()

    fun register(name: String, type: String, port: Int, txt: Map<String, String>) {
        unregister()
        val info = NsdServiceInfo().apply {
            serviceName = name
            serviceType = type
            setPort(port)
            txt.forEach { (key, value) -> setAttribute(key, value) }
        }
        val listener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {}
            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                log("mDNS registration failed (error $errorCode)")
            }
            override fun onServiceUnregistered(info: NsdServiceInfo) {}
            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {}
        }
        registration = listener
        nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
    }

    fun unregister() {
        registration?.let { runCatching { nsd.unregisterService(it) } }
        registration = null
    }

    fun startDiscovery(type: String) {
        stopDiscovery()
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                log("mDNS discovery failed (error $errorCode)")
            }
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}

            override fun onServiceFound(info: NsdServiceInfo) = resolve(info)

            override fun onServiceLost(info: NsdServiceInfo) {
                infoCallbacks.remove(info.serviceName)?.let {
                    runCatching { nsd.unregisterServiceInfoCallback(it) }
                }
                onLost(info.serviceName)
            }
        }
        discovery = listener
        nsd.discoverServices(type, NsdManager.PROTOCOL_DNS_SD, listener)
    }

    fun stopDiscovery() {
        discovery?.let { runCatching { nsd.stopServiceDiscovery(it) } }
        discovery = null
        infoCallbacks.values.forEach { runCatching { nsd.unregisterServiceInfoCallback(it) } }
        infoCallbacks.clear()
    }

    private fun txtOf(info: NsdServiceInfo): Map<String, String> =
        info.attributes.mapValues { (_, value) -> value?.decodeToString() ?: "" }

    private fun resolve(info: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            if (infoCallbacks.containsKey(info.serviceName)) return
            val callback = object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                    infoCallbacks.remove(info.serviceName)
                }
                override fun onServiceUpdated(updated: NsdServiceInfo) {
                    updated.hostAddresses.forEach { host ->
                        onResolved(updated.serviceName, txtOf(updated), host, updated.port)
                    }
                }
                override fun onServiceLost() {}
                override fun onServiceInfoCallbackUnregistered() {}
            }
            infoCallbacks[info.serviceName] = callback
            nsd.registerServiceInfoCallback(info, executor, callback)
        } else {
            executor.execute { resolveLegacy(info) }
        }
    }

    @Suppress("DEPRECATION")
    private fun resolveLegacy(info: NsdServiceInfo) {
        val done = java.util.concurrent.CountDownLatch(1)
        nsd.resolveService(info, object : NsdManager.ResolveListener {
            override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) = done.countDown()
            override fun onServiceResolved(resolved: NsdServiceInfo) {
                resolved.host?.let { onResolved(resolved.serviceName, txtOf(resolved), it, resolved.port) }
                done.countDown()
            }
        })
        done.await(10, java.util.concurrent.TimeUnit.SECONDS)
    }
}
