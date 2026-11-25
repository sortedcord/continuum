package io.github.sortedcord.continuum.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import io.github.sortedcord.continuum.data.NodeRepository
import io.github.sortedcord.continuum.ffi.PairedDevice
import io.github.sortedcord.continuum.ffi.PeerStatusFfi
import io.github.sortedcord.continuum.ffi.ProximityFfi
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(repository: NodeRepository, onPair: () -> Unit, onToggle: (Boolean) -> Unit) {
    val paired by repository.paired.collectAsState()
    val statuses by repository.statuses.collectAsState()
    val running by repository.running.collectAsState()
    val log by repository.log.collectAsState()
    val paging by repository.paging.collectAsState()
    var forgetting by remember { mutableStateOf<PairedDevice?>(null) }
    val scope = rememberCoroutineScope()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Continuum") },
                actions = {
                    Switch(checked = running, onCheckedChange = onToggle, modifier = Modifier.padding(end = 12.dp))
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(onClick = onPair) { Text("Pair device") }
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize().padding(padding).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item {
                Column(Modifier.padding(vertical = 8.dp)) {
                    Text(repository.deviceName, style = MaterialTheme.typography.titleMedium)
                    Text(
                        repository.continuum.deviceId().take(8),
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
            if (paired.isEmpty()) {
                item {
                    Text(
                        "No paired devices yet. On your Linux machine run `continuum pair invite`, then tap Pair device.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
            items(paired, key = { it.id }) { device ->
                DeviceCard(
                    device,
                    statuses[device.id],
                    ringing = device.id in paging,
                    onRing = { ring ->
                        scope.launch {
                            if (ring) {
                                repository.page(device.id).onFailure { repository.log("Couldn't ring: ${it.message}") }
                            } else {
                                repository.stopPage(device.id)
                            }
                        }
                    },
                    onForget = { forgetting = device },
                )
            }
            if (log.isNotEmpty()) {
                item {
                    Text("Activity", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                }
                items(log.take(10)) { line -> Text(line, style = MaterialTheme.typography.bodySmall) }
            }
            item { Spacer(Modifier.padding(40.dp)) }
        }
    }

    forgetting?.let { device ->
        AlertDialog(
            onDismissRequest = { forgetting = null },
            title = { Text("Forget ${device.name}?") },
            text = { Text("${device.name} will forget this phone too. You will need to pair again to reconnect.") },
            confirmButton = {
                TextButton(onClick = {
                    scope.launch { repository.unpair(device.id) }
                    forgetting = null
                }) { Text("Forget") }
            },
            dismissButton = { TextButton(onClick = { forgetting = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun DeviceCard(
    device: PairedDevice,
    status: PeerStatusFfi?,
    ringing: Boolean,
    onRing: (Boolean) -> Unit,
    onForget: () -> Unit,
) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(device.name, style = MaterialTheme.typography.titleMedium)
                    Text(device.shortId, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                }
                if (status?.canPage == true || ringing) {
                    TextButton(onClick = { onRing(!ringing) }) { Text(if (ringing) "Stop" else "Ring") }
                }
                TextButton(onClick = onForget) { Text("Forget") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
                val chips = buildList {
                    if (status?.connected == true) add("Connected")
                    if (status?.reachable == true) add("On network")
                    if (status?.nearby == true) {
                        val band = when (status.proximity) {
                            ProximityFfi.IMMEDIATE -> "Right here"
                            ProximityFfi.NEAR -> "Nearby"
                            ProximityFfi.FAR -> "Far"
                            null -> "In range"
                        }
                        add(status.rssi?.let { "$band · ${it.toInt()} dBm" } ?: band)
                    }
                    if (isEmpty()) add("Not seen")
                }
                chips.forEach { label ->
                    AssistChip(onClick = {}, label = { Text(label) })
                    Spacer(Modifier.width(0.dp))
                }
            }
        }
    }
}
