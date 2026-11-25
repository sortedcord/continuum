package io.github.sortedcord.continuum.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.os.Handler
import android.os.Looper
import android.os.VibrationEffect
import android.os.VibratorManager
import io.github.sortedcord.continuum.R

/**
 * Rings the phone for "Find My Device". Plays on the alarm stream at full
 * volume, so it sounds even when the ringer is on silent, and vibrates.
 * Volume is restored afterwards.
 */
class PageRinger(private val context: Context) {
    private val audio = context.getSystemService(AudioManager::class.java)
    private val vibrator = context.getSystemService(VibratorManager::class.java).defaultVibrator
    private val notifications = context.getSystemService(NotificationManager::class.java)
    private val handler = Handler(Looper.getMainLooper())
    private var player: MediaPlayer? = null
    private var savedVolume: Int? = null
    private var peerId: String? = null

    init {
        notifications.createNotificationChannel(
            NotificationChannel(CHANNEL, "Find my phone", NotificationManager.IMPORTANCE_HIGH).apply {
                setSound(null, null) // We play the sound ourselves.
            },
        )
    }

    val ringing: Boolean get() = player != null

    fun start(peerId: String, peerName: String, seconds: Int) = handler.post {
        stopNow()
        this.peerId = peerId
        savedVolume = audio.getStreamVolume(AudioManager.STREAM_ALARM)
        audio.setStreamVolume(AudioManager.STREAM_ALARM, audio.getStreamMaxVolume(AudioManager.STREAM_ALARM), 0)
        val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
            ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
        player = runCatching {
            MediaPlayer().apply {
                setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_ALARM)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build(),
                )
                setDataSource(context, uri)
                isLooping = true
                prepare()
                start()
            }
        }.getOrNull()
        vibrator.vibrate(VibrationEffect.createWaveform(longArrayOf(0, 800, 400), 0))
        notifications.notify(NOTIFICATION_ID, notification(peerId, peerName))
        handler.postDelayed(::stopNow, seconds * 1000L)
    }

    fun stop() = handler.post(::stopNow)

    /** The user silenced the phone; returns which peer paged it. */
    fun found(): String? {
        val id = peerId
        stop()
        return id
    }

    private fun stopNow() {
        handler.removeCallbacksAndMessages(null)
        player?.runCatching {
            stop()
            release()
        }
        player = null
        vibrator.cancel()
        savedVolume?.let { audio.setStreamVolume(AudioManager.STREAM_ALARM, it, 0) }
        savedVolume = null
        notifications.cancel(NOTIFICATION_ID)
        peerId = null
    }

    private fun notification(peerId: String, peerName: String): Notification {
        val found = PendingIntent.getService(
            context,
            2,
            Intent(context, ContinuumService::class.java)
                .setAction(ContinuumService.ACTION_FOUND)
                .putExtra(ContinuumService.EXTRA_PEER, peerId),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("$peerName is looking for this phone")
            .setContentText("Tap to stop ringing")
            .setCategory(Notification.CATEGORY_ALARM)
            .setContentIntent(found)
            .setDeleteIntent(found)
            .addAction(Notification.Action.Builder(null, "Found it", found).build())
            .setOngoing(true)
            .build()
    }

    private companion object {
        const val CHANNEL = "page"
        const val NOTIFICATION_ID = 2
    }
}
