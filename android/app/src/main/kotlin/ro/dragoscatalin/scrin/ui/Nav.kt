package ro.dragoscatalin.scrin.ui

import androidx.navigation3.runtime.NavKey
import kotlinx.serialization.Serializable

@Serializable sealed interface Route : NavKey
@Serializable data object Home : Route
@Serializable data object Connecting : Route
@Serializable data object Viewer : Route
@Serializable data object Host : Route
@Serializable data object Settings : Route
@Serializable data object TrustedDevices : Route
@Serializable data object About : Route
@Serializable data object RestrictedGuide : Route
