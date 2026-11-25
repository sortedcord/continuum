package io.github.sortedcord.continuum.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Small files encrypted with an AES-256-GCM key that never leaves the
 * Android Keystore. Stored in `noBackupFilesDir`, so secrets are never
 * included in cloud backups.
 *
 * File format: 12-byte IV, then ciphertext with the 16-byte GCM tag.
 */
class SecureStore(context: Context) {
    private val dir = File(context.noBackupFilesDir, "continuum").apply { mkdirs() }

    private val key: SecretKey by lazy {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey) ?: KeyGenerator
            .getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
            .apply {
                init(
                    KeyGenParameterSpec.Builder(
                        KEY_ALIAS,
                        KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                    )
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setKeySize(256)
                        .build(),
                )
            }
            .generateKey()
    }

    fun read(name: String): ByteArray? {
        val file = File(dir, name)
        if (!file.exists()) return null
        val bytes = file.readBytes()
        if (bytes.size < IV_LEN) return null
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, bytes, 0, IV_LEN))
        return cipher.doFinal(bytes, IV_LEN, bytes.size - IV_LEN)
    }

    fun write(name: String, plaintext: ByteArray) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val out = cipher.iv + cipher.doFinal(plaintext)
        // Write-then-rename so a crash never leaves a truncated file.
        val tmp = File(dir, "$name.tmp")
        tmp.writeBytes(out)
        tmp.renameTo(File(dir, name))
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "continuum-storage-v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_LEN = 12
        const val TAG_BITS = 128
    }
}
