## Default Permission

Default permission set for the Psycheros mic-capture plugin. Allows JS to start
and stop native audio capture — required for voice chat inside the Tauri desktop
app on macOS Tahoe where WKWebView doesn't expose navigator.mediaDevices.

#### This default permission set includes the following:

- `allow-start-capture`
- `allow-stop-capture`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>

<tr>
<td>

`psycheros-mic-capture:allow-start-capture`

</td>
<td>

Enables the start_capture command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`psycheros-mic-capture:deny-start-capture`

</td>
<td>

Denies the start_capture command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`psycheros-mic-capture:allow-stop-capture`

</td>
<td>

Enables the stop_capture command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`psycheros-mic-capture:deny-stop-capture`

</td>
<td>

Denies the stop_capture command without any pre-configured scope.

</td>
</tr>
</table>
