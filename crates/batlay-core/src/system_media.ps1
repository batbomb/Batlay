$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime | Out-Null
  $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
  })[0]

  function Await($WinRtTask, $ResultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    return $netTask.Result
  }

  # Sortie : JSON -> octets UTF-8 -> Base64 (ASCII pur). Indépendant de la
  # page de code de la console : plus aucun "�" possible sur les accents.
  function Emit($obj) {
    $json = $obj | ConvertTo-Json -Compress -Depth 4
    [Console]::Out.Write([Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($json)))
  }

  [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager,Windows.Media.Control,ContentType=WindowsRuntime] | Out-Null

  $manager = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
  $sessions = $manager.GetSessions()

  if ($__params.listOnly) {
    $list = @()
    foreach ($s in $sessions) {
      $p = Await ($s.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
      $list += [PSCustomObject]@{ sourceAppId = $s.SourceAppUserModelId; title = $p.Title; artist = $p.Artist }
    }
    Emit ([PSCustomObject]@{ supported = $true; sessions = $list })
    exit 0
  }

  # SELECTION MANUELLE : quand l'utilisateur a explicitement choisi un
  # lecteur, on ne bascule JAMAIS sur un autre. Si ce lecteur n'est pas
  # dans les sessions actives (fermé, en veille, entre deux morceaux), on
  # renvoie session = $null avec preferredMissing = $true : le provider
  # conserve le choix et attend le retour du lecteur. GetCurrentSession()
  # n'est utilisé QUE si aucun lecteur n'a été choisi.
  $session = $null
  $preferredMissing = $false
  if ($__params.preferredAppId) {
    $session = $sessions | Where-Object { $_.SourceAppUserModelId -eq $__params.preferredAppId } | Select-Object -First 1
    if (-not $session) { $preferredMissing = $true }
  } else {
    $session = $manager.GetCurrentSession()
  }

  if (-not $session) {
    Emit ([PSCustomObject]@{ supported = $true; session = $null; preferredMissing = $preferredMissing })
    exit 0
  }

  # État de lecture et timeline lus AVANT l'appel asynchrone des métadonnées
  # (qui prend quelques ms) : la position et son horodatage sont ainsi
  # mesurés au même instant.
  $playback = $session.GetPlaybackInfo()
  $timeline = $session.GetTimelineProperties()
  $sampledAtMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()

  # Timeline.Position est la position au moment de LastUpdatedTime (dernière
  # mise à jour PAR L'APP), pas la position "en direct" : la plupart des
  # lecteurs ne la rafraîchissent qu'au play/pause/seek. Lire Position telle
  # quelle donnait une valeur périmée, corrigée d'un coup (souvent en
  # arrière) quand l'app finissait par la mettre à jour.
  $lastUpdatedMs = $null
  try {
    $lu = $timeline.LastUpdatedTime
    if ($lu -and $lu.Year -ge 2000) { $lastUpdatedMs = $lu.ToUnixTimeMilliseconds() }
  } catch { $lastUpdatedMs = $null }

  $props = Await ($session.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])

  # Fenêtre de Spotify : son titre (« Artiste - Titre », « Advertisement » pendant
  # une pub) est mis à jour tout de suite, alors que SMTC peut rester sur le
  # morceau précédent. Lu UNIQUEMENT pour Spotify (coût : un Get-Process) ; vide
  # si la fenêtre est masquée (réduite dans la zone de notification).
  $windowTitle = $null
  if ($session.SourceAppUserModelId -match 'spotify') {
    try {
      $sp = Get-Process -Name 'Spotify' -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle } | Select-Object -First 1
      if ($sp) { $windowTitle = $sp.MainWindowTitle }
    } catch { $windowTitle = $null }
  }

  $artworkDataUri = $null
  $artworkError = $null
  $hasThumbnail = [bool]$props.Thumbnail
  if ($__params.includeArtwork -and $props.Thumbnail) {
    try {
      [Windows.Storage.Streams.DataReader,Windows.Storage.Streams,ContentType=WindowsRuntime] | Out-Null
      [Windows.Storage.Streams.IRandomAccessStreamWithContentType,Windows.Storage.Streams,ContentType=WindowsRuntime] | Out-Null
      $stream = Await ($props.Thumbnail.OpenReadAsync()) ([Windows.Storage.Streams.IRandomAccessStreamWithContentType])
      $size = [int]$stream.Size
      # Garde-fou : une pochette ne devrait jamais dépasser quelques Mo ;
      # on préfère ne pas l'afficher plutôt que de bloquer sur un flux aberrant.
      if ($size -gt 0 -and $size -lt 5000000) {
        $reader = New-Object Windows.Storage.Streams.DataReader($stream)
        Await ($reader.LoadAsync([uint32]$size)) ([uint32]) | Out-Null
        $bytes = New-Object byte[] $size
        $reader.ReadBytes($bytes)
        $b64 = [Convert]::ToBase64String($bytes)
        $ct = $stream.ContentType
        if ([string]::IsNullOrWhiteSpace($ct)) { $ct = "image/png" }
        $artworkDataUri = "data:$ct;base64,$b64"
      }
    } catch {
      # Remontée volontaire du message d'erreur (plutôt qu'un simple null
      # silencieux) : c'est ce qui a permis de diagnostiquer et corriger
      # un premier bug (type WinRT IRandomAccessStreamWithContentType non
      # enregistré). Le renderer le journalise en console sans bloquer
      # l'affichage du titre/artiste pour autant.
      $artworkDataUri = $null
      $artworkError = $_.Exception.Message
    }
  }

  # PlaybackStatus WinRT : 0 Closed, 1 Opened, 2 Changing, 3 Stopped, 4 Playing, 5 Paused.
  # On distingue explicitement lecture / pause / arrêt plutôt qu'un simple booléen.
  $status = switch ([int]$playback.PlaybackStatus) {
    4 { 'playing' }
    5 { 'paused' }
    3 { 'stopped' }
    default { 'unknown' }
  }

  $result = [PSCustomObject]@{
    supported = $true
    preferredMissing = $false
    session   = [PSCustomObject]@{
      title         = $props.Title
      artist        = $props.Artist
      albumArtist   = $props.AlbumArtist
      album         = $props.AlbumTitle
      windowTitle   = $windowTitle
      sourceAppId   = $session.SourceAppUserModelId
      isPlaying     = ([int]$playback.PlaybackStatus -eq 4)
      playbackStatus = $status
      positionMs    = [math]::Round($timeline.Position.TotalMilliseconds)
      lastUpdatedMs = $lastUpdatedMs
      sampledAtMs   = $sampledAtMs
      durationMs    = [math]::Round($timeline.EndTime.TotalMilliseconds)
      artwork       = $artworkDataUri
      hasThumbnail  = $hasThumbnail
      artworkError  = $artworkError
    }
  }
  Emit $result
} catch {
  Emit ([PSCustomObject]@{ supported = $false; error = $_.Exception.Message })
}
