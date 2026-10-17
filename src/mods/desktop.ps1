$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
$parameters = $request.params
switch ($request.method) {
    'dialogs.openFile' {
        Add-Type -AssemblyName System.Windows.Forms
        $dialog = [System.Windows.Forms.OpenFileDialog]::new()
        $dialog.Title = [string]$parameters.title
        $dialog.Multiselect = [bool]$parameters.multiple
        if ($parameters.extensions) {
            $extensions = @($parameters.extensions | ForEach-Object { if ($_ -notmatch '^[a-zA-Z0-9]+$') { throw 'Invalid file extension' }; '*.' + $_ })
            $dialog.Filter = 'Selected files|' + ($extensions -join ';') + '|All files|*.*'
        }
        try { $result = @(); if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { $result = @($dialog.FileNames) }; ConvertTo-Json -InputObject $result -Compress }
        finally { $dialog.Dispose() }
    }
    'dialogs.openFolder' {
        Add-Type -AssemblyName System.Windows.Forms
        $dialog = [System.Windows.Forms.FolderBrowserDialog]::new()
        $dialog.Description = [string]$parameters.title
        try { $result = $null; if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { $result = $dialog.SelectedPath }; ConvertTo-Json -InputObject $result -Compress }
        finally { $dialog.Dispose() }
    }
    'clipboard.readText' { ConvertTo-Json -InputObject ([string](Get-Clipboard -Raw)) -Compress }
    'clipboard.writeText' { if ($null -eq $parameters.text) { throw 'Text must be a string' }; Set-Clipboard -Value ([string]$parameters.text); 'null' }
    default { throw 'Unknown desktop method' }
}
