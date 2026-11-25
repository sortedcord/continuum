package io.github.sortedcord.continuum.platform

import android.Manifest
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.ParcelUuid
import androidx.core.content.ContextCompat

/**
 * BLE beacon advertising and scanning.
 *
 * The beacon is a non-connectable legacy advertisement holding only service
 * data under the Continuum UUID, exactly 31 bytes on air. The scan filter
 * matches that UUID and the beacon version nibble, which also keeps
 * scanning alive with the screen off.
 */
class BleAdapter(
    private val context: Context,
    private val log: (String) -> Unit,
) {
    private val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
    private var advertiseCallback: AdvertiseCallback? = null
    private var scanCallback: ScanCallback? = null

    private fun granted(permission: String) =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

    val available: Boolean
        get() = adapter?.isEnabled == true

    @Suppress("MissingPermission") // Checked by granted().
    fun advertise(uuid: String, serviceData: ByteArray) {
        stopAdvertising()
        val advertiser = adapter?.bluetoothLeAdvertiser ?: return
        if (!granted(Manifest.permission.BLUETOOTH_ADVERTISE)) {
            log("BLE advertising needs the Nearby devices permission")
            return
        }
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_BALANCED)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(false)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceData(ParcelUuid.fromString(uuid), serviceData)
            .build()
        val callback = object : AdvertiseCallback() {
            override fun onStartFailure(errorCode: Int) {
                log("BLE advertising failed (error $errorCode)")
            }
        }
        advertiseCallback = callback
        advertiser.startAdvertising(settings, data, callback)
    }

    @Suppress("MissingPermission")
    fun stopAdvertising() {
        val callback = advertiseCallback ?: return
        advertiseCallback = null
        if (granted(Manifest.permission.BLUETOOTH_ADVERTISE)) {
            adapter?.bluetoothLeAdvertiser?.stopAdvertising(callback)
        }
    }

    @Suppress("MissingPermission")
    fun startScan(uuid: String, onBeacon: (ByteArray, Int) -> Unit) {
        stopScan()
        val scanner = adapter?.bluetoothLeScanner ?: return
        if (!granted(Manifest.permission.BLUETOOTH_SCAN)) {
            log("BLE scanning needs the Nearby devices permission")
            return
        }
        val parcelUuid = ParcelUuid.fromString(uuid)
        // Match beacon version 1 in the high nibble of the first byte.
        val filter = ScanFilter.Builder()
            .setServiceData(parcelUuid, byteArrayOf(0x10), byteArrayOf(0xF0.toByte()))
            .build()
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_BALANCED)
            .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
            .build()
        val callback = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                val data = result.scanRecord?.getServiceData(parcelUuid) ?: return
                onBeacon(data, result.rssi)
            }

            override fun onBatchScanResults(results: MutableList<ScanResult>) {
                results.forEach { onScanResult(ScanSettings.CALLBACK_TYPE_ALL_MATCHES, it) }
            }

            override fun onScanFailed(errorCode: Int) {
                log("BLE scan failed (error $errorCode)")
            }
        }
        scanCallback = callback
        scanner.startScan(listOf(filter), settings, callback)
    }

    @Suppress("MissingPermission")
    fun stopScan() {
        val callback = scanCallback ?: return
        scanCallback = null
        if (granted(Manifest.permission.BLUETOOTH_SCAN)) {
            adapter?.bluetoothLeScanner?.stopScan(callback)
        }
    }
}
