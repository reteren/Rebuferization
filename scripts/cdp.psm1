# CDP (Chrome DevTools Protocol) client for the popup WebView2 page.
# Usage:
#   Import-Module .\cdp.psm1
#   $c = New-Cdp 'popup'            # attaches to the page whose url ends with index.html
#   $c.Eval('document.body.innerText')
#   $c.EvalScript(...)              # multi-statement script, returns last expression
#   $c.ConsoleDrain()               # returns collected console/exception messages
#   $c.Dispose()
$ErrorActionPreference = 'Stop'

function New-Cdp {
    param([string]$PageMatch = 'index.html')
    $version = Invoke-RestMethod 'http://127.0.0.1:9222/json/version' -ErrorAction Stop
    $targets = Invoke-RestMethod 'http://127.0.0.1:9222/json/list'
    $page = $targets | Where-Object { $_.type -eq 'page' -and $_.url -like "*$PageMatch*" } | Select-Object -First 1
    if (-not $page) { throw "no page target matching $PageMatch; targets: $($targets | ForEach-Object { $_.url })" }

    $ws = [System.Net.WebSockets.ClientWebSocket]::new()
    $ct = [System.Threading.CancellationToken]::None
    $ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, $ct).GetAwaiter().GetResult()

    $script:nextId = 1
    $script:console = New-Object System.Collections.Generic.List[string]
    $script:wsConn = $ws

    $send = {
        param([string]$method, $params)
        $id = $script:nextId++
        $msg = @{ id = $id; method = $method }
        if ($null -ne $params) { $msg.params = $params }
        $json = $msg | ConvertTo-Json -Depth 8 -Compress
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
        $seg = [System.ArraySegment[byte]]::new($bytes)
        $script:wsConn.SendAsync($seg, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $ct).GetAwaiter().GetResult()
        while ($true) {
            $buf = New-Object byte[] 4194304
            $seg2 = [System.ArraySegment[byte]]::new($buf)
            $res = $script:wsConn.ReceiveAsync($seg2, $ct).GetAwaiter().GetResult()
            $txt = [System.Text.Encoding]::UTF8.GetString($buf, 0, $res.Count)
            $obj = $txt | ConvertFrom-Json
            if ($obj.id -eq $id) {
                if ($obj.error) { throw "CDP error: $($obj.error | ConvertTo-Json -Compress)" }
                return $obj.result
            }
            if ($obj.method -eq 'Runtime.consoleAPICalled') {
                $script:console.Add("console: " + (($obj.params.args | ForEach-Object { $_.value }) -join ' '))
            }
            if ($obj.method -eq 'Runtime.exceptionThrown') {
                $d = $obj.params.exceptionDetails
                $script:console.Add("EXCEPTION: " + $d.text + " " + ($d.exception.description ?? ''))
            }
            if ($obj.method -eq 'Log.entryAdded') {
                $script:console.Add("log: " + $obj.params.entry.text)
            }
        }
    }

    $null = & $send 'Runtime.enable' $null
    $null = & $send 'Log.enable' $null
    $null = & $send 'Page.enable' $null

    return [pscustomobject]@{
        Send = $send
        Eval = {
            param([string]$expr)
            $r = & $send 'Runtime.evaluate' @{ expression = $expr; returnByValue = $true; awaitPromise = $true }
            if ($r.exceptionDetails) { throw "eval exception: $($r.exceptionDetails.text) $($r.exceptionDetails.exception.description)" }
            return $r.result.value
        }
        ConsoleDrain = { return ,@($script:console) }
        Dispose = {
            if ($script:wsConn.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
                $script:wsConn.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, 'done', $ct).GetAwaiter().GetResult()
            }
            $script:wsConn.Dispose()
        }
    }
}