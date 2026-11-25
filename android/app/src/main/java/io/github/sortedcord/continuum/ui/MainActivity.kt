package io.github.sortedcord.continuum.ui

import android.Manifest
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import io.github.sortedcord.continuum.ContinuumApp
import io.github.sortedcord.continuum.service.ContinuumService

class MainActivity : ComponentActivity() {
    private val repository by lazy { (application as ContinuumApp).repository }

    private val permissions = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) {
        // Start regardless: without Bluetooth permissions we run on Wi-Fi only.
        ContinuumService.start(this)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        permissions.launch(
            arrayOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.POST_NOTIFICATIONS,
            ),
        )
        setContent {
            ContinuumTheme {
                var pairing by rememberSaveable { mutableStateOf(false) }
                if (pairing) {
                    PairScreen(repository, onDone = { pairing = false })
                } else {
                    HomeScreen(
                        repository,
                        onPair = { pairing = true },
                        onToggle = { run ->
                            if (run) ContinuumService.start(this) else ContinuumService.stop(this)
                        },
                    )
                }
            }
        }
    }
}
