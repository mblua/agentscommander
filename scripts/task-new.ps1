param(
  [Parameter(Mandatory = $true)]
  [string]$Workgroup,

  [Parameter(Mandatory = $true)]
  [string]$IssueOwner,

  [Parameter(Mandatory = $true)]
  [string]$IssueRepo,

  [Parameter(Mandatory = $true)]
  [int]$IssueNumber,

  [Parameter(Mandatory = $true)]
  [string]$Slug,

  [Parameter(Mandatory = $true)]
  [string]$Summary,

  [Parameter(Mandatory = $true)]
  [string]$Branch
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Fail {
  param([string]$Message)
  Write-Error $Message
  exit 1
}

function Write-Utf8NoBom {
  param(
    [string]$Path,
    [string]$Content
  )

  $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Assert-ValidWorkgroup {
  param([string]$Name)
  if ($Name -notmatch '^wg-[a-z0-9][a-z0-9-]*$') {
    Fail "Workgroup '$Name' must match ^wg-[a-z0-9][a-z0-9-]*$."
  }
}

function Assert-ValidSlug {
  param([string]$Name)
  if ($Name -notmatch '^[a-z0-9][a-z0-9-]*$') {
    Fail "Slug '$Name' must use lowercase letters, digits, and hyphens only."
  }
}

function Assert-ValidSummary {
  param([string]$Value)
  if ([string]::IsNullOrWhiteSpace($Value)) {
    Fail "Summary must not be empty or whitespace only."
  }
}

function Assert-ValidBranch {
  param(
    [string]$Name,
    [int]$ExpectedIssueNumber
  )

  $match = [regex]::Match($Name, '^(feature|fix|bug)/[a-z0-9][a-z0-9-]*-gh(\d+)$')
  if (-not $match.Success) {
    Fail "Branch '$Name' must match ^(feature|fix|bug)/[a-z0-9][a-z0-9-]*-gh<number>$."
  }

  $branchIssueNumber = [int]$match.Groups[2].Value
  if ($branchIssueNumber -ne $ExpectedIssueNumber) {
    Fail "Branch '$Name' ends with issue $branchIssueNumber but expected issue $ExpectedIssueNumber."
  }
}

function Get-RepoRoot {
  return (Split-Path -Parent $PSScriptRoot)
}

function Test-CouldMatchActiveTaskForWorkgroup {
  param(
    [string]$Content,
    [string]$WorkgroupName
  )

  $compact = ($Content -replace '\s+', '')
  $escapedWorkgroup = $WorkgroupName.Replace('\', '\\').Replace('"', '\"')
  return $compact.Contains(('\"status\":\"active\",\"activeWorkgroup\":\"{0}\"' -f $escapedWorkgroup))
}

function Read-ExistingTasks {
  param(
    [string]$TasksDir,
    [string]$TargetWorkgroup
  )

  $active = @()
  if (-not (Test-Path -LiteralPath $TasksDir)) {
    return $active
  }

  $jsonFiles = Get-ChildItem -LiteralPath $TasksDir -Filter '*.json' -File | Sort-Object Name
  foreach ($file in $jsonFiles) {
    try {
      $content = Get-Content -Raw -LiteralPath $file.FullName
    } catch {
      Fail "Failed to read existing task file '$($file.FullName)': $($_.Exception.Message)"
    }

    if (-not (Test-CouldMatchActiveTaskForWorkgroup -Content $content -WorkgroupName $TargetWorkgroup)) {
      continue
    }

    try {
      $task = $content | ConvertFrom-Json -Depth 10
    } catch {
      Fail "Failed to parse existing task file '$($file.FullName)': $($_.Exception.Message)"
    }

    if ($null -eq $task.id -or [string]::IsNullOrWhiteSpace([string]$task.id)) {
      Fail "Existing task file '$($file.FullName)' is missing 'id'."
    }
    if ($task.id -ne $file.BaseName) {
      Fail "Existing task file '$($file.FullName)' has id '$($task.id)' but filename stem '$($file.BaseName)'."
    }
    if ($task.status -eq 'active' -and $task.activeWorkgroup -eq $TargetWorkgroup) {
      $active += $file.FullName
    }
  }

  return $active
}

Assert-ValidWorkgroup -Name $Workgroup
Assert-ValidSlug -Name $Slug
Assert-ValidSummary -Value $Summary
Assert-ValidBranch -Name $Branch -ExpectedIssueNumber $IssueNumber

$Summary = $Summary.Trim()

$repoRoot = Get-RepoRoot
$tasksDir = Join-Path $repoRoot '_plans\tasks'
New-Item -ItemType Directory -Force -Path $tasksDir | Out-Null

$lockPath = Join-Path $tasksDir (".lock-$Workgroup.lock")
$lockStream = $null
$jsonTemp = $null
$mdTemp = $null
$jsonFinal = $null
$mdFinal = $null

try {
  $lockStream = [System.IO.File]::Open(
    $lockPath,
    [System.IO.FileMode]::OpenOrCreate,
    [System.IO.FileAccess]::ReadWrite,
    [System.IO.FileShare]::None
  )

  $existingActive = @(Read-ExistingTasks -TasksDir $tasksDir -TargetWorkgroup $Workgroup)
  if ($existingActive.Count -gt 0) {
    Fail "Workgroup '$Workgroup' already has an active task record: $($existingActive -join ', ')"
  }

  $timestamp = Get-Date -Format 'yyyyMMdd_HHmmss'
  $taskId = "$timestamp-$Slug"
  $utcNow = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  $issueUrl = "https://github.com/$IssueOwner/$IssueRepo/issues/$IssueNumber"

  $taskRecord = [ordered]@{
    schemaVersion   = 1
    id              = $taskId
    slug            = $Slug
    summary         = $Summary
    status          = 'active'
    activeWorkgroup = $Workgroup
    workgroupHistory = @(
      [ordered]@{
        workgroup = $Workgroup
        startedAt = $utcNow
        endedAt   = $null
        status    = 'active'
        branch    = $Branch
        note      = 'Initial implementation workgroup.'
      }
    )
    github = [ordered]@{
      owner      = $IssueOwner
      repo       = $IssueRepo
      issueNumber = $IssueNumber
      issueUrl   = $issueUrl
    }
    branch = [ordered]@{
      name = $Branch
      base = 'main'
    }
    messaging = [ordered]@{
      mode       = 'github-issue-comments'
      notifyWith = 'issue-comment-url'
    }
    createdAt = $utcNow
    updatedAt = $utcNow
  }

  $markdown = @'
# Task: {0}

- Task ID: `{1}`
- Status: `active`
- Active workgroup: `{2}`
- GitHub issue: `{3}/{4}#{5}`
- Branch: `{6}`
- Messaging mode: `github-issue-comments`

## Summary

{0}

## Workgroup History

| Workgroup | Started | Ended | Status | Branch | Note |
|---|---|---|---|---|---|
| {2} | {7} | - | active | {6} | Initial implementation workgroup. |

## Notes

- Created by `scripts/task-new.ps1`
'@ -f $Summary, $taskId, $Workgroup, $IssueOwner, $IssueRepo, $IssueNumber, $Branch, $utcNow

  $jsonFinal = Join-Path $tasksDir "$taskId.json"
  $mdFinal = Join-Path $tasksDir "$taskId.md"
  $jsonTemp = Join-Path $tasksDir ".$taskId.json.tmp"
  $mdTemp = Join-Path $tasksDir ".$taskId.md.tmp"

  if ((Test-Path -LiteralPath $jsonFinal) -or (Test-Path -LiteralPath $mdFinal)) {
    Fail "Task id '$taskId' already exists."
  }

  Write-Utf8NoBom -Path $jsonTemp -Content ($taskRecord | ConvertTo-Json -Depth 10)
  Write-Utf8NoBom -Path $mdTemp -Content $markdown

  try {
    Move-Item -LiteralPath $jsonTemp -Destination $jsonFinal
    Move-Item -LiteralPath $mdTemp -Destination $mdFinal
  } catch {
    if ($jsonFinal -and (Test-Path -LiteralPath $jsonFinal)) {
      Remove-Item -LiteralPath $jsonFinal -Force -ErrorAction SilentlyContinue
    }
    if ($mdFinal -and (Test-Path -LiteralPath $mdFinal)) {
      Remove-Item -LiteralPath $mdFinal -Force -ErrorAction SilentlyContinue
    }
    throw
  }

  Write-Output "Created task record: $taskId"
  Write-Output $jsonFinal
  Write-Output $mdFinal
} finally {
  foreach ($tempPath in @($jsonTemp, $mdTemp)) {
    if ($tempPath -and (Test-Path -LiteralPath $tempPath)) {
      Remove-Item -LiteralPath $tempPath -Force -ErrorAction SilentlyContinue
    }
  }
  if ($lockStream) {
    $lockStream.Dispose()
  }
}
