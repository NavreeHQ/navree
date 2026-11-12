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
import androidx.compose.foundation.text.KeyboardActions
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
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SelectableDates
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
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.contentType
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.Events
import com.navree.client.core.FieldValue
import com.navree.client.design.ChoiceWidget
import com.navree.client.design.ChoicesWidget
import com.navree.client.design.FormRules
import com.navree.client.design.PickerMode
import com.navree.client.design.expressive.ConnectedChoice
import com.navree.client.design.fields
import com.navree.client.design.walk
import com.navree.protocol.Navree
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

private fun support(field: Navree.Field, place: Place): Support = Support(place.errors(field), field.help)

@Composable
private fun SupportText(support: Support, counter: String? = null) {
    Row(Modifier.fillMaxWidth()) {
        Text(support.text ?: "", modifier = Modifier.weight(1f))
        if (counter != null) Text(counter, modifier = Modifier.padding(start = 16.dp))
    }
}

/** Draws a field as the Material control its input calls for. */
@Composable
internal fun FieldView(field: Navree.Field, place: Place, modifier: Modifier = Modifier, asItem: Boolean = false) {
    when (val input = field.input) {
        is Navree.TextInput -> TextFieldView(field, input, place, modifier)
        is Navree.ToggleInput -> if (asItem) ToggleItem(field, input, place, modifier) else ToggleRow(field, input, place, modifier)
        is Navree.ChoiceInput -> ChoiceView(field, input, place, modifier)
        is Navree.ChoicesInput -> ChoicesView(field, input, place, modifier)
        is Navree.DateInput -> DateView(field, input, place, modifier)
        is Navree.SliderInput -> SliderView(field, input, place, modifier)
        // A screen with an input this client cannot draw was shown in the web view instead.
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
private fun autofill(input: Navree.TextInput): ContentType? = when (input.autofill) {
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
        Navree.TextKind.EMAIL -> ContentType.EmailAddress
        Navree.TextKind.PASSWORD -> ContentType.Password
        Navree.TextKind.PHONE -> ContentType.PhoneNumber
        Navree.TextKind.ONE_TIME_CODE -> ContentType.SmsOtpCode
        else -> null
    }
}

/** Takes the focus when the core asks for this field (the first field in error, after a post). */
@Composable
private fun FocusWhenAsked(field: Navree.Field, place: Place, requester: FocusRequester) {
    val ui = LocalUi.current
    val asked = place.screen.focus ?: return
    if (asked.target != field.id) return
    LaunchedEffect(asked.serial) {
        runCatching { requester.requestFocus() }
        ui.send(Events.focused(place.id, asked.serial))
    }
}

/** Whether a field is the last text field of its form, so its keyboard key finishes rather than goes on. */
private fun isLast(field: Navree.Field, place: Place): Boolean {
    val form = place.form ?: return true
    val tree = place.screen.screen ?: return true
    val owner = (sequenceOf(tree.body) + tree.actions.asSequence()).flatMap { it.walk() }
        .filterIsInstance<Navree.Form>().firstOrNull { it.id == form } ?: return true
    return owner.fields().lastOrNull { it.input is Navree.TextInput }?.id == field.id
}

@Composable
private fun TextFieldView(field: Navree.Field, input: Navree.TextInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val focus = LocalFocusManager.current
    val current = (place.value(field) as? FieldValue.Text)?.text ?: input.value
    var shown by remember { mutableStateOf(TextFieldValue(current, TextRange(current.length))) }
    val echo = remember { Echo() }
    if (echo.changedOutside(current, shown.text)) shown = TextFieldValue(current, TextRange(current.length))

    val support = support(field, place)
    val plan = FormRules.keyboard(input.kind)
    val password = input.kind == Navree.TextKind.PASSWORD
    var revealed by rememberSaveable { mutableStateOf(false) }
    val requester = remember { FocusRequester() }
    FocusWhenAsked(field, place, requester)
    val content = autofill(input)
    val errorText = support.errors.joinToString(" ")
    val last = remember(place.screen.content, field.id, place.form) { isLast(field, place) }
    val key = FormRules.imeAction(input.kind, last)
    val maxLength = input.maxLength?.toInt()

    OutlinedTextField(
        value = shown,
        onValueChange = { next ->
            val limited = maxLength?.let { most ->
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
                ui.change(place, field, FieldValue.Text(limited.text))
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
        supportingText = if (support.text != null || maxLength != null) {
            { SupportText(support, maxLength?.let { "${shown.text.codePointCount(0, shown.text.length)}/$it" }) }
        } else {
            null
        },
        isError = support.isError,
        visualTransformation = if (password && !revealed) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(
            capitalization = capitalisation(FormRules.capitalise(input.autofill, plan)),
            autoCorrectEnabled = plan.autoCorrect,
            keyboardType = keyboardType(plan.type),
            imeAction = if (!plan.singleLine) {
                ImeAction.Default
            } else {
                when (key) {
                    "search" -> ImeAction.Search
                    "done" -> ImeAction.Done
                    else -> ImeAction.Next
                }
            },
        ),
        keyboardActions = KeyboardActions(
            onDone = {
                focus.clearFocus()
                place.form?.let { ui.act(place, Navree.Action.Submit()) }
            },
            onSearch = {
                focus.clearFocus()
                place.form?.let { ui.act(place, Navree.Action.Submit()) }
            },
        ),
        singleLine = plan.singleLine,
        minLines = if (plan.singleLine) 1 else 3,
        maxLines = if (plan.singleLine) 1 else 8,
    )
}

@Composable
private fun ToggleItem(field: Navree.Field, input: Navree.ToggleInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val on = (place.value(field) as? FieldValue.Flag)?.on ?: input.on
    val support = support(field, place)
    val switch = input.kind == Navree.ToggleKind.SWITCH
    ListItem(
        headlineContent = { Text(field.label ?: field.name) },
        supportingContent = support.text?.let { text -> { Text(text, color = if (support.isError) MaterialTheme.colorScheme.error else Color.Unspecified) } },
        leadingContent = if (switch) null else ({ Checkbox(checked = on, onCheckedChange = null, enabled = !field.disabled) }),
        trailingContent = if (switch) ({ Switch(checked = on, onCheckedChange = null, enabled = !field.disabled) }) else null,
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        // The whole row is the control: one target, one thing for TalkBack to say.
        modifier = modifier.toggleable(
            value = on,
            enabled = !field.disabled,
            role = if (switch) Role.Switch else Role.Checkbox,
            onValueChange = { ui.change(place, field, FieldValue.Flag(it)) },
        ),
    )
}

/** A switch or a checkbox among other content, where a list item's own insets would push it out of line. */
@Composable
private fun ToggleRow(field: Navree.Field, input: Navree.ToggleInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val on = (place.value(field) as? FieldValue.Flag)?.on ?: input.on
    val support = support(field, place)
    val switch = input.kind == Navree.ToggleKind.SWITCH
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .toggleable(
                value = on,
                enabled = !field.disabled,
                role = if (switch) Role.Switch else Role.Checkbox,
                onValueChange = { ui.change(place, field, FieldValue.Flag(it)) },
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
private fun FieldLabel(field: Navree.Field) {
    FormRules.label(field.label, field.required)?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun FieldSupport(support: Support) {
    support.text?.let {
        Text(it, style = MaterialTheme.typography.bodySmall, color = if (support.isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun ChoiceView(field: Navree.Field, input: Navree.ChoiceInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val value = when (val held = place.value(field)) {
        is FieldValue.Text -> held.text
        FieldValue.None -> null
        else -> input.value
    } ?: input.value
    val support = support(field, place)
    val choose: (Navree.ChoiceOption) -> Unit = { option -> ui.change(place, field, FieldValue.Text(option.value)) }
    when (FormRules.choice(input.kind, input.options.size, ui.window)) {
        ChoiceWidget.ButtonGroup -> Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            FieldLabel(field)
            ConnectedChoice(
                labels = input.options.map { it.label },
                selected = input.options.indexOfFirst { it.value == value },
                enabled = { index -> !field.disabled && !input.options[index].disabled },
                onSelect = { index -> choose(input.options[index]) },
            )
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
                            .selectable(selected = option.value == value, enabled = !field.disabled && !option.disabled, role = Role.RadioButton, onClick = { choose(option) }),
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
            PickerField(field, input.options.firstOrNull { it.value == value }?.label ?: "", input.placeholder, support, { Icon(Glyphs.ExpandMore, contentDescription = null) }, { open = true }, modifier)
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
                    confirmButton = { TextButton(onClick = { open = false }) { Text(stringResource(android.R.string.cancel)) } },
                )
            }
        }
    }
}

/** A field that is filled by picking, not typing: it reads like a text field and opens its picker when touched. */
@Composable
private fun PickerField(
    field: Navree.Field,
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
        Box(Modifier.matchParentSize().clickable(enabled = !field.disabled, role = Role.Button, onClickLabel = description, onClick = onOpen))
    }
}

@Composable
private fun ChoicesView(field: Navree.Field, input: Navree.ChoicesInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val chosen = (place.value(field) as? FieldValue.Many)?.values ?: input.values
    val support = support(field, place)
    fun toggle(option: Navree.ChoiceOption) {
        val next = if (option.value in chosen) chosen - option.value else chosen + option.value
        // Sent in the order the options are listed, as a browser posts them.
        ui.change(place, field, FieldValue.Many(input.options.map { it.value }.filter { it in next }))
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
                        leadingIcon = if (selected) ({ Icon(Glyphs.Check, contentDescription = null, modifier = Modifier.size(18.dp)) }) else null,
                    )
                }
            }
            FieldSupport(support)
        }
        ChoicesWidget.Dialog -> {
            var open by remember { mutableStateOf(false) }
            PickerField(field, input.options.filter { it.value in chosen }.joinToString(", ") { it.label }, null, support, { Icon(Glyphs.ExpandMore, contentDescription = null) }, { open = true }, modifier)
            if (open) {
                AlertDialog(
                    onDismissRequest = { open = false },
                    title = field.label?.let { label -> { Text(label) } },
                    text = {
                        LazyColumn {
                            items(input.options) { option ->
                                val selected = option.value in chosen
                                Row(
                                    Modifier.fillMaxWidth().heightIn(min = 56.dp)
                                        .toggleable(value = selected, enabled = !option.disabled, role = Role.Checkbox, onValueChange = { toggle(option) }),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Checkbox(checked = selected, onCheckedChange = null)
                                    Text(option.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 16.dp))
                                }
                            }
                        }
                    },
                    confirmButton = { TextButton(onClick = { open = false }) { Text(stringResource(android.R.string.ok)) } },
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
private fun DateView(field: Navree.Field, input: Navree.DateInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val value = (place.value(field) as? FieldValue.Text)?.text ?: input.value
    val support = support(field, place)
    val date = parseDate(value)
    val time = parseTime(value)
    val mode = FormRules.picker(ui.window)
    var pickDate by remember { mutableStateOf(false) }
    var pickTime by remember { mutableStateOf(false) }
    val dateText = remember(date) { date?.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)) ?: "" }
    val timeText = remember(time) { time?.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)) ?: "" }
    val offDays = remember(input.offDays) { input.offDays.mapNotNull(::parseDate).toSet() }
    val offWeekdays = remember(input.offWeekdays) { input.offWeekdays.map { it.ordinal + 1 }.toSet() }

    fun put(newDate: LocalDate?, newTime: LocalTime?) {
        val text = when (input.kind) {
            Navree.DateKind.DATE -> newDate?.toString() ?: ""
            Navree.DateKind.TIME -> newTime?.let { "%02d:%02d".format(it.hour, it.minute) } ?: ""
            Navree.DateKind.DATETIME -> {
                val day = newDate ?: LocalDate.now()
                val clock = newTime ?: LocalTime.of(9, 0)
                "$day" + "T" + "%02d:%02d".format(clock.hour, clock.minute)
            }
        }
        ui.change(place, field, FieldValue.Text(text))
    }

    val chooseDate = stringResource(R.string.navree_pick_date)
    val chooseTime = stringResource(R.string.navree_pick_time)
    when (input.kind) {
        Navree.DateKind.DATE -> PickerField(field, dateText, null, support, { Icon(Glyphs.Calendar, contentDescription = null) }, { pickDate = true }, modifier, description = chooseDate)
        Navree.DateKind.TIME -> PickerField(field, timeText, null, support, { Icon(Glyphs.Clock, contentDescription = null) }, { pickTime = true }, modifier, description = chooseTime)
        Navree.DateKind.DATETIME -> Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            PickerField(field, dateText, null, support, { Icon(Glyphs.Calendar, contentDescription = null) }, { pickDate = true }, Modifier.weight(3f), description = chooseDate)
            PickerField(field, timeText, null, Support(emptyList(), null), { Icon(Glyphs.Clock, contentDescription = null) }, { pickTime = true }, Modifier.weight(2f), label = chooseTime, description = chooseTime)
        }
    }

    if (pickDate) {
        val least = parseDate(input.min)
        val most = parseDate(input.max)
        val state = rememberDatePickerState(
            initialSelectedDateMillis = date?.let(::millis),
            initialDisplayMode = if (mode == PickerMode.Input) DisplayMode.Input else DisplayMode.Picker,
            selectableDates = remember(least, most, offDays, offWeekdays) {
                object : SelectableDates {
                    override fun isSelectableDate(utcTimeMillis: Long): Boolean {
                        val day = Instant.ofEpochMilli(utcTimeMillis).atZone(ZoneOffset.UTC).toLocalDate()
                        return (least == null || !day.isBefore(least)) && (most == null || !day.isAfter(most)) &&
                            day !in offDays && day.dayOfWeek.value !in offWeekdays
                    }

                    override fun isSelectableYear(year: Int): Boolean = (least == null || year >= least.year) && (most == null || year <= most.year)
                }
            },
        )
        DatePickerDialog(
            onDismissRequest = { pickDate = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        pickDate = false
                        state.selectedDateMillis?.let { picked -> put(Instant.ofEpochMilli(picked).atZone(ZoneOffset.UTC).toLocalDate(), time) }
                    },
                ) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = { TextButton(onClick = { pickDate = false }) { Text(stringResource(android.R.string.cancel)) } },
        ) {
            DatePicker(state = state)
        }
    }

    if (pickTime) {
        val context = LocalContext.current
        val state = rememberTimePickerState(initialHour = time?.hour ?: 9, initialMinute = time?.minute ?: 0, is24Hour = DateFormat.is24HourFormat(context))
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
            dismissButton = { TextButton(onClick = { pickTime = false }) { Text(stringResource(android.R.string.cancel)) } },
            title = { Text(field.label ?: chooseTime) },
        ) {
            if (mode == PickerMode.Input) TimeInput(state = state) else TimePicker(state = state)
        }
    }
}

@Composable
private fun SliderView(field: Navree.Field, input: Navree.SliderInput, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val least = input.min.toDoubleOrNull() ?: 0.0
    val most = (input.max.toDoubleOrNull() ?: 100.0).coerceAtLeast(least)
    val stored = (place.value(field) as? FieldValue.Text)?.text?.toDoubleOrNull() ?: input.value.toDoubleOrNull() ?: least
    var shown by remember(stored) { mutableFloatStateOf(stored.toFloat()) }
    val support = support(field, place)
    val step = input.step?.toDoubleOrNull()
    val steps = if (step != null && step > 0 && most > least) (((most - least) / step).toInt() - 1).coerceIn(0, 100) else 0
    val said = number(shown.toDouble())
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { FieldLabel(field) }
            Text(said, style = MaterialTheme.typography.labelLarge)
        }
        Slider(
            value = shown,
            onValueChange = { shown = it },
            onValueChangeFinished = { ui.change(place, field, FieldValue.Text(number(shown.toDouble()))) },
            valueRange = least.toFloat()..most.toFloat(),
            steps = steps,
            enabled = !field.disabled,
            modifier = Modifier.semantics { field.label?.let { contentDescription = it } },
        )
        FieldSupport(support)
    }
}

/** A number as the web writes it in a form: no trailing `.0`. */
private fun number(value: Double): String = if (value == Math.floor(value) && !value.isInfinite()) value.toLong().toString() else value.toString()
