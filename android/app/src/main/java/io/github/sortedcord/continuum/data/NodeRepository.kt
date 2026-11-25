package io.github.sortedcord.continuum.data

import android.content.Context
import android.os.Build
import android.provider.Settings
import android.util.Log
import io.github.sortedcord.continuum.ffi.AdvertisementFfi
import io.github.sortedcord.continuum.ffi.CodeConfirmer
import io.github.sortedcord.continuum.ffi.Continuum
import io.github.sortedcord.continuum.ffi.DeviceKindFfi
import io.github.sortedcord.continuum.ffi.NodeEventFfi
import io.github.sortedcord.continuum.ffi.NodeListener
import io.github.sortedcord.continuum.ffi.PairedDevice
import io.github.sortedcord.continuum.ffi.PeerStatusFfi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/** A code-pairing prompt waiting for the user. */
data class CodePrompt(val code: String, val peerName: String, val answer: CompletableFuture<Boolean>)

/**
 * Owns the Rust [Continuum] node and exposes its state to the UI.
 * Platform radios are driven by the service through [listener].
 */
class NodeRepository(private val context: Context) {
    private val store = SecureStore(context)

    val deviceName: String = Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
        ?.takeIf { it.isNotBlank() } ?: Build.MODEL

    val continuum: Continuum by lazy { load() }

    private val _paired = MutableStateFlow<List<PairedDevice>>(emptyList())
    val paired: StateFlow<List<PairedDevice>> = _paired.asStateFlow()

    private val _statuses = MutableStateFlow<Map<String, PeerStatusFfi>>(emptyMap())
    val statuses: StateFlow<Map<String, PeerStatusFfi>> = _statuses.asStateFlow()

    private val _candidates = MutableStateFlow<Set<String>>(emptySet())
    /** `ip:port` of devices on the LAN that are waiting to be paired. */
    val candidates: StateFlow<Set<String>> = _candidates.asStateFlow()

    private val _running = MutableStateFlow(false)
    val running: StateFlow<Boolean> = _running.asStateFlow()

    private val _log = MutableStateFlow<List<String>>(emptyList())
    val log: StateFlow<List<String>> = _log.asStateFlow()

    private val _paging = MutableStateFlow<Set<String>>(emptySet())
    /** Peers we are currently making ring. */
    val paging: StateFlow<Set<String>> = _paging.asStateFlow()

    private val _codePrompt = MutableStateFlow<CodePrompt?>(null)
    val codePrompt: StateFlow<CodePrompt?> = _codePrompt.asStateFlow()

    /** Set by the service so advertisements reach the radios. */
    @Volatile
    var onAdvertise: ((AdvertisementFfi) -> Unit)? = null

    /** Set by the service to ring this phone when a peer pages it. */
    @Volatile
    var onPage: ((NodeEventFfi) -> Unit)? = null

    private val prefs = context.getSharedPreferences("continuum", Context.MODE_PRIVATE)

    /** Whether the user wants the service running (also used at boot). */
    var enabled: Boolean
        get() = prefs.getBoolean("enabled", true)
        set(value) = prefs.edit().putBoolean("enabled", value).apply()

    private fun load(): Continuum {
        val kind = if (context.resources.configuration.smallestScreenWidthDp >= 600) {
            DeviceKindFfi.TABLET
        } else {
            DeviceKindFfi.PHONE
        }
        val identity = runCatching { store.read(IDENTITY) }.getOrNull()
        val trust = runCatching { store.read(TRUST) }.getOrNull()
        val node = Continuum(identity, trust, deviceName, kind)
        if (identity == null) store.write(IDENTITY, node.identitySecret())
        _paired.value = node.pairedDevices()
        return node
    }

    fun log(message: String) {
        Log.i(TAG, message)
        _log.update { (listOf(message) + it).take(50) }
    }

    private fun saveTrust() {
        store.write(TRUST, continuum.trustStore())
        _paired.value = continuum.pairedDevices()
    }

    private fun saveIdentity() {
        store.write(IDENTITY, continuum.identitySecret())
    }

    val listener = object : NodeListener {
        override fun onEvent(event: NodeEventFfi) {
            when (event) {
                is NodeEventFfi.Advertise -> onAdvertise?.invoke(event.advertisement)
                is NodeEventFfi.Peer -> _statuses.update { it + (event.status.id to event.status) }
                is NodeEventFfi.SessionUp -> log("Connected to ${event.name}")
                is NodeEventFfi.SessionDown -> log("Disconnected: ${event.reason}")
                is NodeEventFfi.PairingCandidate -> event.addr?.let { addr ->
                    _candidates.update { if (event.present) it + addr else it - addr }
                }
                is NodeEventFfi.TrustChanged -> saveTrust()
                is NodeEventFfi.Unpaired -> {
                    saveTrust()
                    _statuses.update { it - event.peerId }
                    log("${event.name} unpaired this phone")
                }
                is NodeEventFfi.PageStart, is NodeEventFfi.PageStop -> onPage?.invoke(event)
                is NodeEventFfi.PageFound -> {
                    _paging.update { it - event.peerId }
                    log("${event.name} was found")
                }
            }
        }
    }

    fun setRunning(running: Boolean) {
        _running.value = running
        if (!running) _statuses.value = emptyMap()
    }

    suspend fun joinInvite(text: String): Result<PairedDevice> = withContext(Dispatchers.IO) {
        runCatching { continuum.joinInvite(text.trim()) }
            .onSuccess {
                saveTrust()
                log("Paired with ${it.name}")
            }
    }

    suspend fun joinWithCode(addr: String): Result<PairedDevice> = withContext(Dispatchers.IO) {
        val confirmer = object : CodeConfirmer {
            override fun confirm(code: String, peerName: String): Boolean {
                val prompt = CodePrompt(code, peerName, CompletableFuture())
                _codePrompt.value = prompt
                return try {
                    prompt.answer.get(2, TimeUnit.MINUTES)
                } catch (_: Exception) {
                    false
                } finally {
                    _codePrompt.value = null
                }
            }
        }
        runCatching { continuum.joinWithCode(addr, confirmer) }
            .onSuccess {
                saveTrust()
                log("Paired with ${it.name}")
            }
    }

    /** Unpairs on both sides and rotates our presence key. */
    suspend fun unpair(id: String) = withContext(Dispatchers.IO) {
        continuum.unpair(id)
        saveIdentity()
        saveTrust()
        _statuses.update { it - id }
    }

    suspend fun page(id: String, seconds: Int = 30): Result<Unit> = withContext(Dispatchers.IO) {
        runCatching { continuum.page(id, seconds.toUInt()) }.onSuccess {
            _paging.update { it + id }
            kotlinx.coroutines.delay(seconds * 1000L)
            _paging.update { it - id }
        }
    }

    suspend fun stopPage(id: String) = withContext(Dispatchers.IO) {
        _paging.update { it - id }
        runCatching { continuum.stopPage(id) }
    }

    private companion object {
        const val TAG = "Continuum"
        const val IDENTITY = "identity.bin"
        const val TRUST = "trust.bin"
    }
}
