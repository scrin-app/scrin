package ro.dragoscatalin.scrin.tv

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Button
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.OutlinedButton
import androidx.tv.material3.Text
import androidx.tv.material3.darkColorScheme
import app.scrin.brand.BrandDark

class TvActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val scheme = darkColorScheme(
                primary = BrandDark.accentSolid,
                onPrimary = BrandDark.fgOnAccent,
                background = BrandDark.bgCanvas,
                onBackground = BrandDark.fgDefault,
                surface = BrandDark.bgSurface,
                onSurface = BrandDark.fgDefault,
                onSurfaceVariant = BrandDark.fgMuted,
                border = BrandDark.borderFocus,
            )
            MaterialTheme(colorScheme = scheme) {
                TvHome()
            }
        }
    }
}

/** D-pad-first home: every control is focusable and the first one takes focus. */
@Composable
private fun TvHome() {
    var status by remember { mutableStateOf<Int?>(null) }
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    Column(
        Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).padding(horizontal = 64.dp, vertical = 48.dp),
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.displayMedium, color = MaterialTheme.colorScheme.primary)
        Text(stringResource(R.string.tv_tagline), style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onBackground)
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(onClick = { status = R.string.tv_connect_soon }, modifier = Modifier.focusRequester(first)) {
                Text(stringResource(R.string.tv_connect))
            }
            OutlinedButton(onClick = { status = R.string.tv_host_unsupported }) { Text(stringResource(R.string.tv_share_screen)) }
        }
        status?.let { Text(stringResource(it), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(720.dp)) }
    }
}
