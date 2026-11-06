; Batlay 0.5 : identité de l'installateur et du désinstallateur.
;
; Ce fichier est inclus par le modèle NSIS de Tauri AVANT ses pages : les
; réglages de Modern UI ci-dessous s'appliquent donc à l'accueil, à la fin de
; l'installation et à la confirmation de désinstallation. Les images (panneau
; latéral, bannière) et les icônes sont dans tauri.conf.json.
;
; Les couleurs de fond restent celles de Windows : sur fond sombre, le texte
; des cases à cocher (« Lancer Batlay ») resterait noir avec les styles visuels.

; Bannière des pages intérieures, à droite (elle porte le logo et la version).
!define MUI_HEADERIMAGE_RIGHT

!define MUI_WELCOMEPAGE_TITLE "Bienvenue dans Batlay"
!define MUI_WELCOMEPAGE_TEXT "Cet assistant installe Batlay : vos overlays musique, chat et alertes pour le streaming.$\r$\n$\r$\nSi une version précédente est installée, elle est remplacée. Vos overlays, alertes et réglages sont conservés.$\r$\n$\r$\nCliquez sur Suivant pour continuer."

!define MUI_FINISHPAGE_TITLE "Batlay est prêt"
!define MUI_FINISHPAGE_TEXT "Batlay est installé sur cet ordinateur.$\r$\n$\r$\nAjoutez vos overlays dans OBS, TikTok LIVE Studio ou Streamlabs : tout part de la Vue d'ensemble."

!define MUI_UNCONFIRMPAGE_TEXT_TOP "Batlay va être retiré de cet ordinateur. Vos overlays et réglages restent dans %APPDATA%\Batlay, sauf si vous cochez la case ci-dessous."

; Fermer l'assistant demande confirmation (pas de sortie par erreur au milieu).
!define MUI_ABORTWARNING
!define MUI_ABORTWARNING_TEXT "Voulez-vous vraiment quitter l'installation de Batlay ?"

; Batlay 0.5.0 : passage d'Electron à Tauri.
;
; Avant de copier les fichiers, l'installateur retire l'ANCIENNE version
; (Electron, 0.4.x et avant), installée par défaut dans
; %LOCALAPPDATA%\Programs\batlay, avec son propre désinstallateur, en silence.
; Les réglages, overlays et images (%APPDATA%\Batlay) ne sont PAS touchés
; (l'ancien désinstallateur ne les supprime pas : deleteAppDataOnUninstall
; valait false) : la 0.5.0 les reprend tels quels.
;
; Rien d'autre n'est modifié. Sans ancienne version, ce bloc ne fait rien.

!macro BATLAY_UNINSTALL_ELECTRON_AT DIR
  ; DIR = dossier d'une ancienne installation Electron (contient « Uninstall Batlay.exe »).
  ${If} ${FileExists} "${DIR}\Uninstall Batlay.exe"
    DetailPrint "Retrait de l'ancienne version de Batlay (${DIR})…"
    ClearErrors
    ; _?= : attend la fin du désinstallateur (sinon il se relance depuis %TEMP% et rend la main tout de suite).
    ExecWait '"${DIR}\Uninstall Batlay.exe" /S _?=${DIR}' $R3
    ${IfNot} ${Errors}
    ${AndIf} $R3 == 0
      ; Avec _?=, le désinstallateur ne s'efface pas lui-même : on finit le ménage.
      Delete "${DIR}\Uninstall Batlay.exe"
      RMDir "${DIR}"
    ${Else}
      DetailPrint "L'ancienne version n'a pas pu être retirée (code $R3)."
      ${IfNot} ${Silent}
        MessageBox MB_OK|MB_ICONINFORMATION "Une ancienne version de Batlay n'a pas pu être retirée automatiquement ($\"${DIR}$\").$\r$\n$\r$\nVos réglages sont conservés. Vous pourrez la désinstaller ensuite depuis Paramètres > Applications."
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4

  ; 1. Dossier par défaut de l'ancienne version (installation « pour moi »).
  !insertmacro BATLAY_UNINSTALL_ELECTRON_AT "$LOCALAPPDATA\Programs\batlay"

  ; 2. Dossier choisi à l'installation : lu dans la liste des programmes de
  ;    l'utilisateur. Seules les entrées dont le dossier contient
  ;    « Uninstall Batlay.exe » (nom propre à l'ancienne version) sont retenues :
  ;    celle de Batlay 0.5 (uninstall.exe) n'est jamais concernée.
  StrCpy $R0 0
  ${Do}
    EnumRegKey $R1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall" $R0
    ${If} $R1 == ""
      ${Break}
    ${EndIf}
    IntOp $R0 $R0 + 1
    ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$R1" "InstallLocation"
    ; Retire d'éventuels guillemets autour du chemin.
    StrCpy $R4 $R2 1
    ${If} $R4 == '"'
      StrCpy $R2 $R2 "" 1
      StrCpy $R2 $R2 -1
    ${EndIf}
    ${If} $R2 != ""
    ${AndIf} ${FileExists} "$R2\Uninstall Batlay.exe"
      !insertmacro BATLAY_UNINSTALL_ELECTRON_AT "$R2"
      ; La liste a pu changer (entrée retirée) : on la reprend depuis le début.
      ; Une entrée qui n'a pas pu être retirée garde son désinstallateur… et
      ; serait revue sans fin : on ne recommence que si elle a bien disparu.
      ${IfNot} ${FileExists} "$R2\Uninstall Batlay.exe"
        StrCpy $R0 0
      ${EndIf}
    ${EndIf}
  ${Loop}

  ; 3. Ancienne installation « pour tous les utilisateurs » : elle demande les
  ;    droits administrateur, que cet installateur (par utilisateur) n'a pas.
  ${If} ${FileExists} "$PROGRAMFILES64\Batlay\Uninstall Batlay.exe"
  ${AndIfNot} ${Silent}
    MessageBox MB_OK|MB_ICONINFORMATION "Une ancienne version de Batlay est installée pour tous les utilisateurs ($PROGRAMFILES64\Batlay).$\r$\n$\r$\nVos réglages sont conservés. Pensez à la désinstaller depuis Paramètres > Applications."
  ${EndIf}

  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
!macroend
