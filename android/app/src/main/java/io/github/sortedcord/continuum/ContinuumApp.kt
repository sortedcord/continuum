package io.github.sortedcord.continuum

import android.app.Application
import io.github.sortedcord.continuum.data.NodeRepository

class ContinuumApp : Application() {
    val repository: NodeRepository by lazy { NodeRepository(this) }
}
