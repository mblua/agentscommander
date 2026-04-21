param(
  [Parameter(Mandatory = $true)]
  [string]$Root,

  [Parameter(Mandatory = $true)]
  [string]$BodyFile,

  [string]$To,

  [ValidateSet('wake', 'active-only', 'wake-and-sleep')]
  [string]$Mode = 'wake',

  [string]$Token,

  [string]$BinaryPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Fail {
  param([string]$Message)
  Write-Error $Message
  exit 1
}

function Get-OptionalStringProperty {
  param(
    $Object,
    [string]$PropertyName
  )

  $property = $Object.PSObject.Properties[$PropertyName]
  if ($null -eq $property -or $null -eq $property.Value) {
    return $null
  }

  return [string]$property.Value
}

function Get-RequiredStringProperty {
  param(
    $Object,
    [string]$PropertyName,
    [string]$Context
  )

  $value = Get-OptionalStringProperty -Object $Object -PropertyName $PropertyName
  if ([string]::IsNullOrWhiteSpace($value)) {
    Fail "$Context is missing '$PropertyName'."
  }

  return $value
}

function Get-RequiredObjectProperty {
  param(
    $Object,
    [string]$PropertyName,
    [string]$Context
  )

  $property = $Object.PSObject.Properties[$PropertyName]
  if ($null -eq $property -or $null -eq $property.Value) {
    Fail "$Context is missing '$PropertyName'."
  }

  return $property.Value
}

function Get-RequiredArrayProperty {
  param(
    $Object,
    [string]$PropertyName,
    [string]$Context
  )

  $property = $Object.PSObject.Properties[$PropertyName]
  if ($null -eq $property -or $null -eq $property.Value) {
    Fail "$Context is missing '$PropertyName'."
  }

  $values = @($property.Value)
  if ($values.Count -eq 0) {
    Fail "$Context has an empty '$PropertyName' collection."
  }

  return $values
}

function Get-RequiredIntProperty {
  param(
    $Object,
    [string]$PropertyName,
    [string]$Context
  )

  $property = $Object.PSObject.Properties[$PropertyName]
  if ($null -eq $property -or $null -eq $property.Value) {
    Fail "$Context is missing '$PropertyName'."
  }

  $parsed = 0
  if (-not [int]::TryParse([string]$property.Value, [ref]$parsed)) {
    Fail "$Context has a non-integer '$PropertyName' value '$($property.Value)'."
  }

  return $parsed
}

function Test-ContainsJsonStringProperty {
  param(
    [string]$Content,
    [string]$PropertyName,
    [string]$Value
  )

  $compact = ($Content -replace '\s+', '')
  $escapedValue = $Value.Replace('\', '\\').Replace('"', '\"')
  return $compact.Contains(('"{0}":"{1}"' -f $PropertyName, $escapedValue))
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

function Assert-ValidBranchName {
  param(
    [string]$Name,
    [int]$ExpectedIssueNumber,
    [string]$TaskPath
  )

  $match = [regex]::Match($Name, '^(feature|fix|bug)/[a-z0-9][a-z0-9-]*-gh(\d+)$')
  if (-not $match.Success) {
    Fail "Branch '$Name' must match ^(feature|fix|bug)/[a-z0-9][a-z0-9-]*-gh<number>$. ($TaskPath)"
  }

  $branchIssueNumber = [int]$match.Groups[2].Value
  if ($branchIssueNumber -ne $ExpectedIssueNumber) {
    Fail "Branch '$Name' ends with issue $branchIssueNumber but expected issue $ExpectedIssueNumber. ($TaskPath)"
  }
}

function Test-IsRelevantActiveTask {
  param(
    $Task,
    [string]$WorkgroupName
  )

  $status = Get-OptionalStringProperty -Object $Task -PropertyName 'status'
  $activeWorkgroup = Get-OptionalStringProperty -Object $Task -PropertyName 'activeWorkgroup'
  return $status -eq 'active' -and $activeWorkgroup -eq $WorkgroupName
}

function Assert-ActiveTaskRecord {
  param(
    $Task,
    [string]$TaskPath,
    [string]$WorkgroupName
  )

  $taskContext = "Task file '$TaskPath'"
  $taskId = Get-RequiredStringProperty -Object $Task -PropertyName 'id' -Context $taskContext
  if ($taskId -ne [System.IO.Path]::GetFileNameWithoutExtension($TaskPath)) {
    Fail "$taskContext has id '$taskId' but the filename stem does not match."
  }

  $slug = Get-RequiredStringProperty -Object $Task -PropertyName 'slug' -Context $taskContext
  if ($slug -notmatch '^[a-z0-9][a-z0-9-]*$') {
    Fail "$taskContext has invalid slug '$slug'."
  }
  $null = Get-RequiredStringProperty -Object $Task -PropertyName 'summary' -Context $taskContext

  $status = Get-RequiredStringProperty -Object $Task -PropertyName 'status' -Context $taskContext
  $activeWorkgroup = Get-RequiredStringProperty -Object $Task -PropertyName 'activeWorkgroup' -Context $taskContext
  if ($status -ne 'active') {
    Fail "$taskContext must use status 'active'."
  }
  if ($activeWorkgroup -ne $WorkgroupName) {
    Fail "$taskContext has activeWorkgroup '$activeWorkgroup' but expected '$WorkgroupName'."
  }

  $github = Get-RequiredObjectProperty -Object $Task -PropertyName 'github' -Context $taskContext
  $githubContext = "$taskContext github"
  $owner = Get-RequiredStringProperty -Object $github -PropertyName 'owner' -Context $githubContext
  $repo = Get-RequiredStringProperty -Object $github -PropertyName 'repo' -Context $githubContext
  $issueNumber = Get-RequiredIntProperty -Object $github -PropertyName 'issueNumber' -Context $githubContext
  $issueUrl = Get-RequiredStringProperty -Object $github -PropertyName 'issueUrl' -Context $githubContext
  $expectedIssueUrl = "https://github.com/$owner/$repo/issues/$issueNumber"
  if ($issueUrl -ne $expectedIssueUrl) {
    Fail "$taskContext has issueUrl '$issueUrl' but expected '$expectedIssueUrl'."
  }

  $branch = Get-RequiredObjectProperty -Object $Task -PropertyName 'branch' -Context $taskContext
  $branchContext = "$taskContext branch"
  $branchName = Get-RequiredStringProperty -Object $branch -PropertyName 'name' -Context $branchContext
  Assert-ValidBranchName -Name $branchName -ExpectedIssueNumber $issueNumber -TaskPath $TaskPath

  $messaging = Get-RequiredObjectProperty -Object $Task -PropertyName 'messaging' -Context $taskContext
  $messagingContext = "$taskContext messaging"
  $mode = Get-RequiredStringProperty -Object $messaging -PropertyName 'mode' -Context $messagingContext
  $notifyWith = Get-RequiredStringProperty -Object $messaging -PropertyName 'notifyWith' -Context $messagingContext
  if ($mode -ne 'github-issue-comments' -or $notifyWith -ne 'issue-comment-url') {
    Fail "$taskContext must use messaging.mode='github-issue-comments' and notifyWith='issue-comment-url'."
  }

  $history = Get-RequiredArrayProperty -Object $Task -PropertyName 'workgroupHistory' -Context $taskContext
  $openRows = @($history | Where-Object { $null -eq $_.endedAt })
  if ($openRows.Count -ne 1) {
    Fail "$taskContext must have exactly one workgroupHistory row with endedAt = null."
  }

  $openRow = $openRows[0]
  $historyContext = "$taskContext open workgroupHistory row"
  $openWorkgroup = Get-RequiredStringProperty -Object $openRow -PropertyName 'workgroup' -Context $historyContext
  $openStatus = Get-RequiredStringProperty -Object $openRow -PropertyName 'status' -Context $historyContext
  $openBranch = Get-RequiredStringProperty -Object $openRow -PropertyName 'branch' -Context $historyContext
  if ($openWorkgroup -ne $activeWorkgroup) {
    Fail "$taskContext has activeWorkgroup '$activeWorkgroup' but open workgroupHistory row workgroup '$openWorkgroup'."
  }
  if ($openStatus -ne 'active') {
    Fail "$taskContext has open workgroupHistory row status '$openStatus' instead of 'active'."
  }
  if ($openBranch -ne $branchName) {
    Fail "$taskContext has branch '$branchName' but open workgroupHistory row branch '$openBranch'."
  }
}

function Resolve-WorkgroupRoot {
  param([string]$AgentRoot)

  $resolved = [System.IO.Path]::GetFullPath($AgentRoot)
  $agentDir = Split-Path -Leaf $resolved
  if ($agentDir -notlike '__agent_*') {
    Fail "Root '$resolved' must point to a workgroup replica directory named __agent_*."
  }

  $workgroupDir = Split-Path -Parent $resolved
  $workgroupName = Split-Path -Leaf $workgroupDir
  if ($workgroupName -notmatch '^wg-[a-z0-9][a-z0-9-]*$') {
    Fail "Root '$resolved' must be inside a workgroup directory named wg-*."
  }

  return [ordered]@{
    RootPath      = $resolved
    WorkgroupDir  = $workgroupDir
    WorkgroupName = $workgroupName
  }
}

function Read-TaskRecordFile {
  param(
    [string]$TaskPath,
    [string]$WorkgroupName
  )

  try {
    $content = Get-Content -Raw -LiteralPath $TaskPath
  } catch {
    Fail "Failed to read task file '$TaskPath': $($_.Exception.Message)"
  }

  try {
    $task = $content | ConvertFrom-Json -Depth 10
  } catch {
    if (Test-CouldMatchActiveTaskForWorkgroup -Content $content -WorkgroupName $WorkgroupName) {
      Fail "Failed to parse task file '$TaskPath': $($_.Exception.Message)"
    }
    return $null
  }

  if (-not (Test-IsRelevantActiveTask -Task $task -WorkgroupName $WorkgroupName)) {
    return $null
  }

  Assert-ActiveTaskRecord -Task $task -TaskPath $TaskPath -WorkgroupName $WorkgroupName
  return $task
}

function Resolve-ActiveTask {
  param(
    [string]$WorkgroupDir,
    [string]$WorkgroupName
  )

  $repoDirs = Get-ChildItem -LiteralPath $WorkgroupDir -Directory |
    Where-Object { $_.Name -like 'repo-*' } |
    Sort-Object Name

  $tasks = @()
  foreach ($repoDir in $repoDirs) {
    $tasksDir = Join-Path $repoDir.FullName '_plans\tasks'
    if (-not (Test-Path -LiteralPath $tasksDir)) {
      continue
    }

    Get-ChildItem -LiteralPath $tasksDir -Filter '*.json' -File | Sort-Object Name | ForEach-Object {
      $task = Read-TaskRecordFile -TaskPath $_.FullName -WorkgroupName $WorkgroupName
      if ($null -ne $task) {
        $tasks += [ordered]@{
          RepoRoot = $repoDir.FullName
          TaskPath = $_.FullName
          Task     = $task
        }
      }
    }
  }

  if ($tasks.Count -eq 0) {
    Fail "No active task record was found for workgroup '$WorkgroupName'."
  }
  if ($tasks.Count -gt 1) {
    $paths = $tasks | ForEach-Object { $_.TaskPath }
    Fail "Expected exactly one active task record for workgroup '$WorkgroupName' but found $($tasks.Count): $($paths -join ', ')"
  }

  return $tasks[0]
}

function Assert-BranchMatchesTask {
  param(
    [string]$RepoRoot,
    [string]$ExpectedBranch
  )

  $branch = (& git -C $RepoRoot rev-parse --abbrev-ref HEAD).Trim()
  if ($LASTEXITCODE -ne 0) {
    Fail "Failed to read the current git branch in '$RepoRoot'."
  }
  if ($branch -ne $ExpectedBranch) {
    Fail "Current branch '$branch' does not match active task branch '$ExpectedBranch'."
  }
}

$workgroupContext = Resolve-WorkgroupRoot -AgentRoot $Root

if ($To) {
  if ([string]::IsNullOrWhiteSpace($Token) -or [string]::IsNullOrWhiteSpace($BinaryPath)) {
    Fail "Notify mode requires both -Token and -BinaryPath before posting the GitHub comment."
  }
  if (-not (Test-Path -LiteralPath $BinaryPath)) {
    Fail "BinaryPath '$BinaryPath' does not exist."
  }
}

if (-not (Test-Path -LiteralPath $BodyFile)) {
  Fail "Body file '$BodyFile' does not exist."
}

$body = Get-Content -Raw -LiteralPath $BodyFile
if ([string]::IsNullOrWhiteSpace($body)) {
  Fail "Body file '$BodyFile' is empty."
}

$activeTask = Resolve-ActiveTask -WorkgroupDir $workgroupContext.WorkgroupDir -WorkgroupName $workgroupContext.WorkgroupName
Assert-BranchMatchesTask -RepoRoot $activeTask.RepoRoot -ExpectedBranch $activeTask.Task.branch.name

$ghCommand = Get-Command gh -ErrorAction SilentlyContinue
if (-not $ghCommand) {
  Fail "GitHub CLI ('gh') is not installed or not on PATH."
}

& gh auth status | Out-Null
if ($LASTEXITCODE -ne 0) {
  Fail "GitHub CLI is not authenticated. Run 'gh auth status' and fix authentication first."
}

$endpoint = "repos/$($activeTask.Task.github.owner)/$($activeTask.Task.github.repo)/issues/$($activeTask.Task.github.issueNumber)/comments"
$responseJson = & gh api $endpoint --method POST --field "body=$body"
if ($LASTEXITCODE -ne 0) {
  Fail "Failed to create the GitHub issue comment via 'gh api'."
}

try {
  $response = $responseJson | ConvertFrom-Json -Depth 10
} catch {
  Fail "Failed to parse the GitHub API response: $($_.Exception.Message)"
}

$commentUrl = [string]$response.html_url
if ([string]::IsNullOrWhiteSpace($commentUrl)) {
  Fail "GitHub API response did not include html_url."
}

Write-Output "Created GitHub comment: $commentUrl"

if ($To) {
  & $BinaryPath send --token $Token --root $workgroupContext.RootPath --to $To --message $commentUrl --mode $Mode
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
  Write-Output "Notified $To with GitHub comment URL."
}
