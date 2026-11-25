package io.github.sortedcord.continuum.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import io.github.sortedcord.continuum.data.NodeRepository
import io.github.sortedcord.continuum.ffi.PairedDevice
import kotlinx.coroutines.launch

private const val INVITE_PREFIX = "continuum:pair:"

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairScreen(repository: NodeRepository, onDone: () -> Unit) {
    BackHandler(onBack = onDone)
    var tab by remember { mutableIntStateOf(0) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val prompt by repository.codePrompt.collectAsState()

    fun finish(result: Result<PairedDevice>) {
        busy = false
        result.onSuccess { onDone() }.onFailure { error = it.message ?: "Pairing failed" }
    }

    val join: (String) -> Unit = { text ->
        if (!busy) {
            busy = true
            error = null
            scope.launch { finish(repository.joinInvite(text)) }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Pair a device") },
                navigationIcon = { TextButton(onClick = onDone) { Text("Back") } },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            PrimaryTabRow(selectedTabIndex = tab) {
                listOf("Scan QR", "Paste", "Nearby").forEachIndexed { index, title ->
                    Tab(selected = tab == index, onClick = { tab = index }, text = { Text(title) })
                }
            }
            Column(
                Modifier.fillMaxSize().padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(
                    "On Linux, run `continuum pair invite` and scan the QR code it shows.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                when {
                    busy -> Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator()
                    }
                    tab == 0 -> ScanTab(onInvite = join)
                    tab == 1 -> PasteTab(onInvite = join)
                    else -> NearbyTab(repository) { addr ->
                        busy = true
                        error = null
                        scope.launch { finish(repository.joinWithCode(addr)) }
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        }
    }

    prompt?.let { p ->
        AlertDialog(
            onDismissRequest = {},
            title = { Text("Pair with ${p.peerName}?") },
            text = {
                Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                    Text("Check that the other device shows the same code:")
                    Text(
                        p.code,
                        fontFamily = FontFamily.Monospace,
                        fontSize = 36.sp,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(top = 16.dp),
                    )
                }
            },
            confirmButton = { TextButton(onClick = { p.answer.complete(true) }) { Text("They match") } },
            dismissButton = { TextButton(onClick = { p.answer.complete(false) }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun ScanTab(onInvite: (String) -> Unit) {
    val context = LocalContext.current
    var granted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED,
        )
    }
    val request = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted = it }
    LaunchedEffect(Unit) { if (!granted) request.launch(Manifest.permission.CAMERA) }
    var wrongCode by remember { mutableStateOf(false) }
    if (granted) {
        QrScanner(Modifier.fillMaxWidth().aspectRatio(1f)) { text ->
            if (text.startsWith(INVITE_PREFIX)) onInvite(text) else wrongCode = true
        }
        if (wrongCode) Text("That QR code isn't a Continuum invite.")
    } else {
        OutlinedButton(onClick = { request.launch(Manifest.permission.CAMERA) }) {
            Text("Allow camera to scan")
        }
    }
}

@Composable
private fun PasteTab(onInvite: (String) -> Unit) {
    var text by remember { mutableStateOf("") }
    OutlinedTextField(
        value = text,
        onValueChange = { text = it },
        label = { Text("Invite (continuum:pair:…)") },
        modifier = Modifier.fillMaxWidth(),
        minLines = 3,
    )
    Button(onClick = { onInvite(text) }, enabled = text.trim().startsWith(INVITE_PREFIX)) {
        Text("Pair")
    }
}

@Composable
private fun NearbyTab(repository: NodeRepository, onPick: (String) -> Unit) {
    val candidates by repository.candidates.collectAsState()
    Text(
        "Devices on this network that are waiting to be paired. You'll compare a six-digit code.",
        style = MaterialTheme.typography.bodySmall,
    )
    if (candidates.isEmpty()) {
        Text("Nothing yet. Make sure both devices are on the same Wi-Fi.")
    }
    candidates.sorted().forEach { addr ->
        OutlinedButton(onClick = { onPick(addr) }, modifier = Modifier.fillMaxWidth()) {
            Text(addr, fontFamily = FontFamily.Monospace)
        }
    }
}
