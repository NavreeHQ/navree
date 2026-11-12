@file:OptIn(ExperimentalLayoutApi::class)

package com.navree.client.ui

import android.text.format.DateFormat
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DisplayMode
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimeInput
import androidx.compose.material3.TimePicker
import androidx.compose.material3.TimePickerDialog
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.ContentType
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.contentType
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.design.ChoiceWidget
import com.navree.client.design.ChoicesWidget
import com.navree.client.design.FormRules
import com.navree.client.design.PickerMode
import com.navree.client.protocol.ChoiceInput
import com.navree.client.protocol.ChoicesInput
import com.navree.client.protocol.DateInput
import com.navree.client.protocol.DateKind
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.Option
import com.navree.client.protocol.SliderInput
import com.navree.client.protocol.TextInput
import com.navree.client.protocol.TextKind
import com.navree.client.protocol.ToggleInput
import com.navree.client.protocol.ToggleKind
import com.navree.client.runtime.Event
import com.navree.client.runtime.FieldValue
import com.navree.client.runtime.Forms
import com.navree.client.runtime.Reducer
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/** What a field shows under itself: its errors if it has any, its help otherwise. */
private class Support(val errors: List<String>, val help: String?) {
    val isError: Boolean get() = errors.isNotEmpty()
    val text: String? get() = if (isError) errors.joinToString(" ") else help
}

@Composable
private fun support(field: FieldNode, place: Place): Support {
    val local = place.entry.local
    val fromOps = local.errors(place.form, field.name)
    // An error that came with the tree stands until the user changes the field.
    val fromTree = if (local.value(place.form, field.name) == null) field.error else emptyList()
    return Support(fromOps.ifEmpty { fromTree }, field.help)
}

private fun change(ui: Ui, place: Place, field: FieldNode, value: FieldValue) {
    ui.dispatch(Event.FieldChanged(place.key, place.form, field.name, value))
}

@Composable
private fun SupportText(support: Support, counter: String? = null) {
    Row(Modifier.fillMaxWidth()) {
        Text(support.text ?: "", modifier = Modifier.weight(1f))
        if (counter != null) Text(counter, modifier = Modifier.padding(start = 16.dp))
    }
}

/** Draws a field as the Material control its input calls for. */
@Composable
internal fun FieldView(field: FieldNode, place: Place, modifier: Modifier = Modifier, asItem: Boolean = false) {
    when (val input = field.input) {
        is TextInput -> TextFieldView(field, input, place, modifier)
        is ToggleInput -> if (asItem) ToggleView(field, input, place, modifier) else ToggleRow(field, input, place, modifier)
        is ChoiceInput -> ChoiceView(field, input, place, modifier)
        is ChoicesInput -> ChoicesView(field, input, place, modifier)
        is DateInput -> DateView(field, input, place, modifier)
        is SliderInput -> SliderView(field, input, place, modifier)
        // A hidden field draws nothing, and a screen with an input this client cannot draw was shown in the web view instead.
        else -> Unit
    }
}

private fun keyboardType(type: String): KeyboardType = when (type) {
    "email" -> KeyboardType.Email
    "password" -> KeyboardType.Password
    "number" -> KeyboardType.Number
    "decimal" -> KeyboardType.Decimal
    "phone" -> KeyboardType.Phone
    "uri" -> KeyboardType.Uri
    else -> KeyboardType.Text
}

private fun capitalisation(name: String): KeyboardCapitalization = when (name) {
    "words" -> KeyboardCapitalization.Words
    "sentences" -> KeyboardCapitalization.Sentences
    else -> KeyboardCapitalization.None
}

/** What the system's autofill may put in a field, from the web's `autocomplete` names. */
private fun autofill(input: TextInput): ContentType? = when (input.autofill) {
    "name" -> ContentType.PersonFullName
    "given-name" -> ContentType.PersonFirstName
    "family-name" -> ContentType.PersonLastName
    "email" -> ContentType.EmailAddress
    "username" -> ContentType.Username
    "current-password" -> ContentType.Password
    "new-password" -> ContentType.NewPassword
    "tel" -> ContentType.PhoneNumber
    "tel-national" -> ContentType.PhoneNumberNational
    "street-address", "address-line1" -> ContentType.AddressStreet
    "address-level2" -> ContentType.AddressLocality
    "address-level1" -> ContentType.AddressRegion
    "postal-code" -> ContentType.PostalCode
    "country", "country-name" -> ContentType.AddressCountry
    "one-time-code" -> ContentType.SmsOtpCode
    "cc-number" -> ContentType.CreditCardNumber
    else -> when (input.kind) {
        TextKind.Email -> ContentType.EmailAddress
        TextKind.Password -> ContentType.Password
        TextKind.Phone -> ContentType.PhoneNumber
        TextKind.OneTimeCode -> ContentType.SmsOtpCode
        else -> null
    }
}

/** Takes the focus when the server, or a failed post, asks for this field. */
@Composable
private fun FocusWhenAsked(field: FieldNode, place: Place, requester: FocusRequester) {
    val ui = LocalUi.current
    val asked = place.entry.local.focus ?: return
    val mine = asked.target == field.id || asked.target == Reducer.fieldTarget(place.form ?: "", field.name)
    if (!mine) return
    LaunchedEffect(asked.serial) {
        requester.requestFocus()
        ui.dispatch(Event.FocusTaken(place.key, asked.serial))
    }
}

@Composable
private fun TextFieldView(field: FieldNode, input: TextInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val current = (place.entry.local.value(place.form, field.name) as? FieldValue.Text)?.text ?: input.value ?: ""
    var shown by remember { mutableStateOf(TextFieldValue(current, TextRange(current.length))) }
    val echo = remember { Echo() }
    if (echo.changedOutside(current, shown.text)) shown = TextFieldValue(current, TextRange(current.length))

    val support = support(field, place)
    val plan = FormRules.keyboard(input.kind)
    val password = input.kind == TextKind.Password
    var revealed by rememberSaveable { mutableStateOf(false) }
    val requester = remember { FocusRequester() }
    FocusWhenAsked(field, place, requester)
    val content = autofill(input)
    val errorText = support.errors.joinToString(" ")

    OutlinedTextField(
        value = shown,
        onValueChange = { next ->
            val limited = input.maxLength?.let { most ->
                if (next.text.codePointCount(0, next.text.length) > most) {
                    val cut = next.text.offsetByCodePoints(0, most)
                    next.copy(text = next.text.substring(0, cut), selection = TextRange(minOf(next.selection.end, cut)))
                } else {
                    next
                }
            } ?: next
            val changed = limited.text != shown.text
            shown = limited
            if (changed) {
                echo.report(limited.text)
                change(ui, place, field, FieldValue.Text(limited.text))
            }
        },
        modifier = modifier
            .fillMaxWidth()
            .focusRequester(requester)
            .semantics {
                if (content != null) contentType = content
                if (support.isError) error(errorText)
            },
        enabled = !field.disabled,
        label = FormRules.label(field.label, field.required)?.let { label -> { Text(label) } },
        placeholder = input.placeholder?.let { placeholder -> { Text(placeholder) } },
        readOnly = input.readonly,
        trailingIcon = if (password) {
            {
                IconButton(onClick = { revealed = !revealed }) {
                    Icon(
                        if (revealed) Glyphs.VisibilityOff else Glyphs.Visibility,
                        contentDescription = stringResource(if (revealed) R.string.navree_hide_password else R.string.navree_show_password),
                    )
                }
            }
        } else if (support.isError) {
            { Icon(Glyphs.Error, contentDescription = null) }
        } else {
            null
        },
        supportingText = if (support.text != null || input.maxLength != null) {
            {
                SupportText(support, input.maxLength?.let { "${shown.text.codePointCount(0, shown.text.length)}/$it" })
            }
        } else {
            null
        },
        isError = support.isError,
        visualTransformation = if (password && !revealed) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(
            capitalization = capitalisation(FormRules.capitalise(input.autofill, plan)),
            autoCorrectEnabled = plan.autoCorrect,
            keyboardType = keyboardType(plan.type),
            imeAction = if (plan.singleLine) ImeAction.Next else ImeAction.Default,
        ),
        singleLine = plan.singleLine,
        minLines = if (plan.singleLine) 1 else 3,
        maxLines = if (plan.singleLine) 1 else 8,
    )
}

@Composable
private fun ToggleView(field: FieldNode, input: ToggleInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val on = (place.entry.local.value(place.form, field.name) as? FieldValue.Flag)?.on ?: input.on
    val support = support(field, place)
    val switch = input.kind == ToggleKind.Switch
    ListItem(
        headlineContent = { Text(field.label ?: field.name) },
        supportingContent = support.text?.let { text ->
            { Text(text, color = if (support.isError) MaterialTheme.colorScheme.error else Color.Unspecified) }
        },
        leadingContent = if (switch) {
            null
        } else {
            { Checkbox(checked = on, onCheckedChange = null, enabled = !field.disabled) }
        },
        trailingContent = if (switch) {
            { Switch(checked = on, onCheckedChange = null, enabled = !field.disabled) }
        } else {
            null
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        // The whole row is the control: one target, one thing for TalkBack to say.
        modifier = modifier.toggleable(
            value = on,
            enabled = !field.disabled,
            role = if (switch) Role.Switch else Role.Checkbox,
            onValueChange = { change(ui, place, field, FieldValue.Flag(it)) },
        ),
    )
}

/** A switch or a checkbox among other content, where a list item's own insets would push it out of line. */
@Composable
private fun ToggleRow(field: FieldNode, input: ToggleInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val on = (place.entry.local.value(place.form, field.name) as? FieldValue.Flag)?.on ?: input.on
    val support = support(field, place)
    val switch = input.kind == ToggleKind.Switch
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .toggleable(
                value = on,
                enabled = !field.disabled,
                role = if (switch) Role.Switch else Role.Checkbox,
                onValueChange = { change(ui, place, field, FieldValue.Flag(it)) },
            ),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!switch) {
            Checkbox(checked = on, onCheckedChange = null, enabled = !field.disabled)
            Box(Modifier.width(16.dp))
        }
        Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
            Text(field.label ?: field.name, style = MaterialTheme.typography.bodyLarge)
            FieldSupport(support)
        }
        if (switch) {
            Box(Modifier.width(16.dp))
            Switch(checked = on, onCheckedChange = null, enabled = !field.disabled)
        }
    }
}

@Composable
private fun FieldLabel(field: FieldNode) {
    FormRules.label(field.label, field.required)?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun FieldSupport(support: Support) {
    support.text?.let {
        Text(
            it,
            style = MaterialTheme.typography.bodySmall,
            color = if (support.isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ChoiceView(field: FieldNode, input: ChoiceInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val value = (place.entry.local.value(place.form, field.name) as? FieldValue.Text)?.text ?: input.value
    val support = support(field, place)
    val choose: (Option) -> Unit = { option -> change(ui, place, field, FieldValue.Text(option.value ?: "")) }
    when (FormRules.choice(input.kind, input.options.size, ui.window)) {
        ChoiceWidget.Segmented -> Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            FieldLabel(field)
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                input.options.forEachIndexed { index, option ->
                    SegmentedButton(
                        selected = option.value == value,
                        onClick = { choose(option) },
                        shape = SegmentedButtonDefaults.itemShape(index = index, count = input.options.size),
                        enabled = !field.disabled && !option.disabled,
                        label = { Text(option.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    )
                }
            }
            FieldSupport(support)
        }
        ChoiceWidget.Radio -> Column(modifier.fillMaxWidth()) {
            FieldLabel(field)
            Column(Modifier.selectableGroup()) {
                for (option in input.options) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .selectable(
                                selected = option.value == value,
                                enabled = !field.disabled && !option.disabled,
                                role = Role.RadioButton,
                                onClick = { choose(option) },
                            ),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = option.value == value, onClick = null)
                        Text(option.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 16.dp))
                    }
                }
            }
            FieldSupport(support)
        }
        ChoiceWidget.Menu -> {
            var open by remember { mutableStateOf(false) }
            ExposedDropdownMenuBox(expanded = open, onExpandedChange = { if (!field.disabled) open = it }, modifier = modifier) {
                OutlinedTextField(
                    value = input.options.firstOrNull { it.value == value }?.label ?: "",
                    onValueChange = {},
                    readOnly = true,
                    enabled = !field.disabled,
                    label = FormRules.label(field.label, field.required)?.let { label -> { Text(label) } },
                    placeholder = input.placeholder?.let { placeholder -> { Text(placeholder) } },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
                    supportingText = support.text?.let { text -> { Text(text) } },
                    isError = support.isError,
                    singleLine = true,
                    modifier = Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable).fillMaxWidth(),
                )
                ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                    for (option in input.options) {
                        DropdownMenuItem(
                            text = { Text(option.label) },
                            onClick = {
                                open = false
                                choose(option)
                            },
                            enabled = !option.disabled,
                            trailingIcon = option.detail?.let { detail -> { Text(detail, style = MaterialTheme.typography.labelMedium) } },
                            contentPadding = ExposedDropdownMenuDefaults.ItemContentPadding,
                        )
                    }
                }
            }
        }
        ChoiceWidget.Dialog -> {
            var open by remember { mutableStateOf(false) }
            PickerField(
                field = field,
                text = input.options.firstOrNull { it.value == value }?.label ?: "",
                placeholder = input.placeholder,
                support = support,
                icon = { Icon(Glyphs.ExpandMore, contentDescription = null) },
                onOpen = { open = true },
                modifier = modifier,
            )
            if (open) {
                AlertDialog(
                    onDismissRequest = { open = false },
                    title = field.label?.let { label -> { Text(label) } },
                    text = {
                        LazyColumn(Modifier.selectableGroup()) {
                            items(input.options) { option ->
                                Row(
                                    Modifier
                                        .fillMaxWidth()
                                        .heightIn(min = 56.dp)
                                        .selectable(
                                            selected = option.value == value,
                                            enabled = !option.disabled,
                                            role = Role.RadioButton,
                                            onClick = {
                                                open = false
                                                choose(option)
                                            },
                                        ),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    RadioButton(selected = option.value == value, onClick = null)
                                    Text(option.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 16.dp))
                                }
                            }
                        }
                    },
                    confirmButton = {
                        TextButton(onClick = { open = false }) { Text(stringResource(android.R.string.cancel)) }
                    },
                )
            }
        }
    }
}

/** A field that is filled by picking, not typing: it reads like a text field and opens its picker when touched. */
@Composable
private fun PickerField(
    field: FieldNode,
    text: String,
    placeholder: String?,
    support: Support,
    icon: @Composable () -> Unit,
    onOpen: () -> Unit,
    modifier: Modifier = Modifier,
    label: String? = field.label,
    description: String? = null,
) {
    Box(modifier) {
        OutlinedTextField(
            value = text,
            onValueChange = {},
            readOnly = true,
            enabled = !field.disabled,
            label = FormRules.label(label, field.required)?.let { shown -> { Text(shown) } },
            placeholder = placeholder?.let { shown -> { Text(shown) } },
            trailingIcon = icon,
            supportingText = support.text?.let { shown -> { Text(shown) } },
            isError = support.isError,
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        // The field itself would open the keyboard; this takes the touch and opens the picker.
        Box(
            Modifier
                .matchParentSize()
                .clickable(enabled = !field.disabled, role = Role.Button, onClickLabel = description, onClick = onOpen),
        )
    }
}

@Composable
private fun ChoicesView(field: FieldNode, input: ChoicesInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val chosen = (place.entry.local.value(place.form, field.name) as? FieldValue.Many)?.values ?: input.values
    val support = support(field, place)
    fun toggle(option: Option) {
        val value = option.value ?: return
        val next = if (value in chosen) chosen - value else chosen + value
        // Posted in the order the options are listed, as a browser posts them.
        change(ui, place, field, FieldValue.Many(input.options.mapNotNull { it.value }.filter { it in next }))
    }
    when (FormRules.choices(input.options.size)) {
        ChoicesWidget.Chips -> Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            FieldLabel(field)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (option in input.options) {
                    val selected = option.value in chosen
                    FilterChip(
                        selected = selected,
                        onClick = { toggle(option) },
                        enabled = !field.disabled && !option.disabled,
                        label = { Text(option.label) },
                        leadingIcon = if (selected) {
                            { Icon(Glyphs.Check, contentDescription = null, modifier = Modifier.size(18.dp)) }
                        } else {
                            null
                        },
                    )
                }
            }
            FieldSupport(support)
        }
        ChoicesWidget.Dialog -> {
            var open by remember { mutableStateOf(false) }
            PickerField(
                field = field,
                text = input.options.filter { it.value in chosen }.joinToString(", ") { it.label },
                placeholder = null,
                support = support,
                icon = { Icon(Glyphs.ExpandMore, contentDescription = null) },
                onOpen = { open = true },
                modifier = modifier,
            )
            if (open) {
                AlertDialog(
                    onDismissRequest = { open = false },
                    title = field.label?.let { label -> { Text(label) } },
                    text = {
                        LazyColumn {
                            items(input.options) { option ->
                                val selected = option.value in chosen
                                Row(
                                    Modifier
                                        .fillMaxWidth()
                                        .heightIn(min = 56.dp)
                                        .toggleable(value = selected, enabled = !option.disabled, role = Role.Checkbox, onValueChange = { toggle(option) }),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Checkbox(checked = selected, onCheckedChange = null)
                                    Text(option.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 16.dp))
                                }
                            }
                        }
                    },
                    confirmButton = {
                        TextButton(onClick = { open = false }) { Text(stringResource(android.R.string.ok)) }
                    },
                )
            }
        }
    }
}

private fun parseDate(text: String?): LocalDate? = try {
    text?.take(10)?.let(LocalDate::parse)
} catch (_: RuntimeException) {
    null
}

private fun parseTime(text: String?): LocalTime? = try {
    text?.let { LocalTime.parse(if (it.contains('T')) it.substringAfter('T') else it) }
} catch (_: RuntimeException) {
    null
}

private fun millis(date: LocalDate): Long = date.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()

@Composable
private fun DateView(field: FieldNode, input: DateInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val value = (place.entry.local.value(place.form, field.name) as? FieldValue.Text)?.text ?: input.value
    val support = support(field, place)
    val date = parseDate(value)
    val time = parseTime(value)
    val mode = FormRules.picker(input.kind, ui.window)
    var pickDate by remember { mutableStateOf(false) }
    var pickTime by remember { mutableStateOf(false) }
    val dateText = remember(date) { date?.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)) ?: "" }
    val timeText = remember(time) { time?.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)) ?: "" }

    fun put(newDate: LocalDate?, newTime: LocalTime?) {
        val text = when (input.kind) {
            DateKind.Date -> newDate?.toString() ?: ""
            DateKind.Time -> newTime?.let { "%02d:%02d".format(it.hour, it.minute) } ?: ""
            DateKind.DateTime -> {
                val day = newDate ?: LocalDate.now()
                val clock = newTime ?: LocalTime.of(9, 0)
                "$day" + "T" + "%02d:%02d".format(clock.hour, clock.minute)
            }
        }
        change(ui, place, field, FieldValue.Text(text))
    }

    val chooseDate = stringResource(R.string.navree_pick_date)
    val chooseTime = stringResource(R.string.navree_pick_time)
    when (input.kind) {
        DateKind.Date -> PickerField(
            field, dateText, null, support, { Icon(Glyphs.Calendar, contentDescription = null) }, { pickDate = true }, modifier,
            description = chooseDate,
        )
        DateKind.Time -> PickerField(
            field, timeText, null, support, { Icon(Glyphs.Clock, contentDescription = null) }, { pickTime = true }, modifier,
            description = chooseTime,
        )
        DateKind.DateTime -> Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            PickerField(
                field, dateText, null, support, { Icon(Glyphs.Calendar, contentDescription = null) }, { pickDate = true },
                Modifier.weight(3f), description = chooseDate,
            )
            PickerField(
                field, timeText, null, Support(emptyList(), null), { Icon(Glyphs.Clock, contentDescription = null) }, { pickTime = true },
                Modifier.weight(2f), label = chooseTime, description = chooseTime,
            )
        }
    }

    if (pickDate) {
        val least = parseDate(input.min)
        val most = parseDate(input.max)
        val state = rememberDatePickerState(
            initialSelectedDateMillis = date?.let(::millis),
            initialDisplayMode = if (mode == PickerMode.Input) DisplayMode.Input else DisplayMode.Picker,
            selectableDates = remember(least, most) {
                object : SelectableDates {
                    override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                        (least == null || utcTimeMillis >= millis(least)) && (most == null || utcTimeMillis <= millis(most))

                    override fun isSelectableYear(year: Int): Boolean =
                        (least == null || year >= least.year) && (most == null || year <= most.year)
                }
            },
        )
        DatePickerDialog(
            onDismissRequest = { pickDate = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        pickDate = false
                        state.selectedDateMillis?.let { picked ->
                            put(Instant.ofEpochMilli(picked).atZone(ZoneOffset.UTC).toLocalDate(), time)
                        }
                    },
                ) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = {
                TextButton(onClick = { pickDate = false }) { Text(stringResource(android.R.string.cancel)) }
            },
        ) {
            DatePicker(state = state)
        }
    }

    if (pickTime) {
        val context = LocalContext.current
        val state = rememberTimePickerState(
            initialHour = time?.hour ?: 9,
            initialMinute = time?.minute ?: 0,
            is24Hour = DateFormat.is24HourFormat(context),
        )
        TimePickerDialog(
            onDismissRequest = { pickTime = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        pickTime = false
                        put(date, LocalTime.of(state.hour, state.minute))
                    },
                ) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = {
                TextButton(onClick = { pickTime = false }) { Text(stringResource(android.R.string.cancel)) }
            },
            title = { Text(field.label ?: chooseTime) },
        ) {
            if (mode == PickerMode.Input) TimeInput(state = state) else TimePicker(state = state)
        }
    }
}

@Composable
private fun SliderView(field: FieldNode, input: SliderInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val least = input.min?.toDoubleOrNull() ?: 0.0
    val most = (input.max?.toDoubleOrNull() ?: 100.0).coerceAtLeast(least)
    val stored = (place.entry.local.value(place.form, field.name) as? FieldValue.Number)?.number
        ?: input.value?.toDoubleOrNull() ?: least
    var shown by remember(stored) { mutableFloatStateOf(stored.toFloat()) }
    val support = support(field, place)
    val step = input.step?.toDoubleOrNull()
    val steps = if (step != null && step > 0 && most > least) (((most - least) / step).toInt() - 1).coerceIn(0, 100) else 0
    val said = Forms.number(shown.toDouble())
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { FieldLabel(field) }
            Text(said, style = MaterialTheme.typography.labelLarge)
        }
        Slider(
            value = shown,
            onValueChange = { shown = it },
            onValueChangeFinished = { change(ui, place, field, FieldValue.Number(shown.toDouble())) },
            valueRange = least.toFloat()..most.toFloat(),
            steps = steps,
            enabled = !field.disabled,
            modifier = Modifier.semantics { field.label?.let { contentDescription = it } },
        )
        FieldSupport(support)
    }
}
