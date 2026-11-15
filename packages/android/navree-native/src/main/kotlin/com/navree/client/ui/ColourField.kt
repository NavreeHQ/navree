@file:OptIn(ExperimentalLayoutApi::class)

package com.navree.client.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.FieldValue
import com.navree.client.design.ColourRules
import com.navree.protocol.Navree
import kotlin.math.roundToInt

/**
 * A colour, as Material 3 has no component for one (its component list has
 * none): a field the way the date field is, showing the colour as a swatch
 * and as its hex value, which opens a dialog to choose in. The dialog has a
 * palette of swatches, sliders for hue, saturation and lightness, and a box
 * for the hex value that checks what is typed. What the field sends is
 * `#rrggbb`, as the protocol says.
 */
@Composable
internal fun ColourFieldView(field: Navree.Field, input: Navree.ColourInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val held = (place.value(field) as? FieldValue.Text)?.text ?: input.value
    val rgb = ColourRules.parse(held)
    val support = support(field, place)
    var open by rememberSaveable { mutableStateOf(false) }
    PickerField(
        field = field,
        // What the page holds is shown as it is when it is not a colour the client can read.
        text = rgb?.let(ColourRules::hex) ?: held,
        placeholder = null,
        support = support,
        icon = { Swatch(rgb, Modifier.size(24.dp)) },
        onOpen = { open = true },
        modifier = modifier,
        description = stringResource(R.string.navree_choose_colour),
    )
    if (open) {
        ColourDialog(
            title = field.label,
            start = rgb,
            onDone = { chosen ->
                open = false
                ui.change(place, field, FieldValue.Text(ColourRules.hex(chosen)))
            },
            onDismiss = { open = false },
        )
    }
}

/** A round swatch of a colour; with no colour it is an outline. [selected] thickens its ring, which is more than a colour to see. */
@Composable
internal fun Swatch(rgb: Int?, modifier: Modifier = Modifier, selected: Boolean = false, content: @Composable BoxScope.() -> Unit = {}) {
    Box(
        modifier
            .background(if (rgb != null) Color(rgb or 0xFF000000.toInt()) else Color.Transparent, CircleShape)
            .border(if (selected) 3.dp else 1.dp, if (selected) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.outline, CircleShape),
        contentAlignment = Alignment.Center,
        content = content,
    )
}

@Composable
private fun familyName(rgb: Int): String = stringResource(
    when (ColourRules.family(rgb)) {
        ColourRules.Family.Black -> R.string.navree_colour_black
        ColourRules.Family.White -> R.string.navree_colour_white
        ColourRules.Family.Grey -> R.string.navree_colour_grey
        ColourRules.Family.Red -> R.string.navree_colour_red
        ColourRules.Family.Orange -> R.string.navree_colour_orange
        ColourRules.Family.Brown -> R.string.navree_colour_brown
        ColourRules.Family.Yellow -> R.string.navree_colour_yellow
        ColourRules.Family.Green -> R.string.navree_colour_green
        ColourRules.Family.Teal -> R.string.navree_colour_teal
        ColourRules.Family.Cyan -> R.string.navree_colour_cyan
        ColourRules.Family.Blue -> R.string.navree_colour_blue
        ColourRules.Family.Purple -> R.string.navree_colour_purple
        ColourRules.Family.Pink -> R.string.navree_colour_pink
    },
)

/** What a screen reader says of a colour: its family and its hex value, as in "Green, #16a34a". */
@Composable
private fun colourName(rgb: Int): String = stringResource(R.string.navree_colour_named, familyName(rgb), ColourRules.hex(rgb))

/**
 * The dialog. The colour being chosen is a draft: a swatch, a slider or the
 * hex box changes it, the others follow, and nothing is sent until OK, which
 * is not offered while the hex box holds something that is not a colour.
 */
@Composable
private fun ColourDialog(title: String?, start: Int?, onDone: (Int) -> Unit, onDismiss: () -> Unit) {
    // Hue, saturation, lightness: kept apart from the colour so that a grey keeps the hue the slider was left at.
    var packed by rememberSaveable {
        val from = start?.let { ColourRules.toHsl(it) } ?: ColourRules.Hsl(0f, 0f, 50f)
        mutableStateOf(floatArrayOf(from.hue, from.saturation, from.lightness))
    }
    var typed by rememberSaveable { mutableStateOf(start?.let(ColourRules::hex) ?: "") }
    val hsl = ColourRules.Hsl(packed[0], packed[1], packed[2])
    val parsed = ColourRules.parse(typed)
    val wrong = typed.isNotEmpty() && parsed == null

    fun set(next: ColourRules.Hsl) {
        packed = floatArrayOf(next.hue, next.saturation, next.lightness)
        typed = ColourRules.hex(ColourRules.fromHsl(next))
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = title?.let { shown -> { Text(shown) } },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                // The colour as it stands: a swatch, and its words.
                // With nothing typed there is no colour yet; with something that is not a colour yet, the one the sliders stand at.
                val previewed: Int? = parsed ?: if (typed.isEmpty()) null else ColourRules.fromHsl(hsl)
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Swatch(previewed, Modifier.size(40.dp))
                    Text(previewed?.let { colourName(it) } ?: stringResource(R.string.navree_colour_none), style = MaterialTheme.typography.titleMedium)
                }
                FlowRow(Modifier.fillMaxWidth().selectableGroup()) {
                    for (swatch in ColourRules.PALETTE) {
                        val chosen = parsed == swatch
                        val name = colourName(swatch)
                        Box(
                            Modifier
                                .size(48.dp)
                                .selectable(selected = chosen, role = Role.RadioButton, onClick = { set(ColourRules.toHsl(swatch, hsl.hue)) })
                                .semantics { contentDescription = name },
                            contentAlignment = Alignment.Center,
                        ) {
                            Swatch(swatch, Modifier.size(40.dp), selected = chosen) {
                                // The chosen one is marked by a tick as well as a ring, never by colour alone.
                                if (chosen) {
                                    Icon(Glyphs.Check, contentDescription = null, tint = Color(ColourRules.onColour(swatch) or 0xFF000000.toInt()), modifier = Modifier.size(24.dp))
                                }
                            }
                        }
                    }
                }
                HslSlider(stringResource(R.string.navree_hue), stringResource(R.string.navree_degrees, hsl.hue.roundToInt()), hsl.hue, 360f) { set(hsl.copy(hue = it)) }
                HslSlider(stringResource(R.string.navree_saturation), stringResource(R.string.navree_percent_value, hsl.saturation.roundToInt()), hsl.saturation, 100f) { set(hsl.copy(saturation = it)) }
                HslSlider(stringResource(R.string.navree_lightness), stringResource(R.string.navree_percent_value, hsl.lightness.roundToInt()), hsl.lightness, 100f) { set(hsl.copy(lightness = it)) }
                val wrongWords = stringResource(R.string.navree_colour_hex_wrong)
                OutlinedTextField(
                    value = typed,
                    onValueChange = { next ->
                        val text = next.take(7)
                        typed = text
                        // A typed colour moves the sliders; what is not a colour yet leaves them where they are.
                        ColourRules.parse(text)?.let { packed = ColourRules.toHsl(it, hsl.hue).let { found -> floatArrayOf(found.hue, found.saturation, found.lightness) } }
                    },
                    label = { Text(stringResource(R.string.navree_colour_hex)) },
                    supportingText = { Text(if (wrong) wrongWords else stringResource(R.string.navree_colour_hex_hint)) },
                    trailingIcon = if (wrong) ({ Icon(Glyphs.Error, contentDescription = null) }) else null,
                    isError = wrong,
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = KeyboardType.Ascii, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { parsed?.let(onDone) }),
                    modifier = Modifier.fillMaxWidth().semantics { if (wrong) error(wrongWords) },
                )
            }
        },
        confirmButton = { TextButton(onClick = { parsed?.let(onDone) }, enabled = parsed != null) { Text(stringResource(android.R.string.ok)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

/** One of the three sliders: its name and value above, and the value said to a screen reader as words, not as a share of the track. */
@Composable
private fun HslSlider(label: String, shown: String, value: Float, most: Float, onChange: (Float) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            Text(shown, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(start = 16.dp))
        }
        Slider(
            value = value,
            onValueChange = onChange,
            valueRange = 0f..most,
            modifier = Modifier.heightIn(min = 48.dp).semantics {
                contentDescription = label
                stateDescription = shown
            },
        )
    }
}
