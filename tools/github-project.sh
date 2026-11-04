#!/usr/bin/env sh
# One-time setup of the OpenPID GitHub Project (Scrum dashboard), see docs/PROCESS.md.
#
# Run it locally as the repository owner:
#   gh auth refresh -s project,read:project     # Projects need the `project` scope
#   sh tools/github-project.sh
#
# What it does: creates the project, the Priority / Size / Area fields, links the
# project to the repository and adds every open issue. What GitHub does not let
# a script do (and you finish in the UI, ~5 minutes): the Sprint iteration
# field, the five views and the built-in workflows listed in docs/PROCESS.md.
set -eu

OWNER="${OWNER:-Rufics}"
REPO="${REPO:-OpenPID}"
TITLE="${TITLE:-OpenPID}"

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }

if ! gh auth status 2>/dev/null | grep -q project; then
  echo "The gh token has no 'project' scope. Run: gh auth refresh -s project,read:project" >&2
  exit 1
fi

say "Project '$TITLE' (owner $OWNER)"
number=$(gh project list --owner "$OWNER" --format json --limit 100 \
  | jq -r --arg t "$TITLE" '.projects[] | select(.title == $t) | .number' | head -n1)
if [ -z "$number" ]; then
  number=$(gh project create --owner "$OWNER" --title "$TITLE" --format json | jq -r .number)
  say "created project #$number"
else
  say "project #$number already exists, updating"
fi

field_exists() { gh project field-list "$number" --owner "$OWNER" --format json | jq -e --arg n "$1" '.fields[] | select(.name == $n)' >/dev/null; }

if ! field_exists Priority; then
  gh project field-create "$number" --owner "$OWNER" --name Priority --data-type SINGLE_SELECT \
    --single-select-options "P0,P1,P2,P3" >/dev/null
  say "field Priority"
fi
if ! field_exists Size; then
  gh project field-create "$number" --owner "$OWNER" --name Size --data-type SINGLE_SELECT \
    --single-select-options "XS,S,M,L,XL" >/dev/null
  say "field Size"
fi
if ! field_exists Area; then
  gh project field-create "$number" --owner "$OWNER" --name Area --data-type SINGLE_SELECT \
    --single-select-options "core,dsp,analysis,sim,gui,cli,fmt-blackbox,fmt-ardupilot,ci,docs" >/dev/null
  say "field Area"
fi

say "linking to $OWNER/$REPO"
gh project link "$number" --owner "$OWNER" --repo "$OWNER/$REPO" >/dev/null 2>&1 || true

say "adding open issues"
project_id=$(gh project view "$number" --owner "$OWNER" --format json | jq -r .id)
existing=$(gh project item-list "$number" --owner "$OWNER" --format json --limit 500 | jq -r '.items[].content.number // empty')
fields=$(gh project field-list "$number" --owner "$OWNER" --format json)
prio_field=$(echo "$fields" | jq -r '.fields[] | select(.name=="Priority") | .id')
area_field=$(echo "$fields" | jq -r '.fields[] | select(.name=="Area") | .id')

gh issue list --repo "$OWNER/$REPO" --state open --limit 500 --json number,url,labels \
  | jq -c '.[]' | while read -r issue; do
  num=$(echo "$issue" | jq -r .number)
  url=$(echo "$issue" | jq -r .url)
  if echo "$existing" | grep -qx "$num"; then
    item_id=$(gh project item-list "$number" --owner "$OWNER" --format json --limit 500 \
      | jq -r --argjson n "$num" '.items[] | select(.content.number == $n) | .id')
  else
    item_id=$(gh project item-add "$number" --owner "$OWNER" --url "$url" --format json | jq -r .id)
    say "added #$num"
  fi
  # mirror the priority / area labels into the fields
  prio=$(echo "$issue" | jq -r '[.labels[].name | select(startswith("priority: "))][0] // empty' | sed 's/priority: //')
  if [ -n "$prio" ]; then
    opt=$(echo "$fields" | jq -r --arg p "$prio" '.fields[] | select(.name=="Priority") | .options[] | select(.name==$p) | .id')
    gh project item-edit --project-id "$project_id" --id "$item_id" --field-id "$prio_field" --single-select-option-id "$opt" >/dev/null
  fi
  area=$(echo "$issue" | jq -r '[.labels[].name | select(startswith("area: "))][0] // empty' | sed 's/area: //')
  if [ -n "$area" ]; then
    opt=$(echo "$fields" | jq -r --arg a "$area" '.fields[] | select(.name=="Area") | .options[] | select(.name==$a) | .id')
    [ -n "$opt" ] && gh project item-edit --project-id "$project_id" --id "$item_id" --field-id "$area_field" --single-select-option-id "$opt" >/dev/null
  fi
done

cat <<EOF

Done. Finish in the browser (https://github.com/users/$OWNER/projects/$number/settings):
  1. Fields: add "Sprint" (Iteration, 2 weeks, starts Monday).
  2. Views (docs/PROCESS.md): Backlog (table, group Milestone), Sprint board (board by Status,
     filter sprint:@current), Roadmap (date field Sprint), Bugs, Blocked.
  3. Workflows: Item added -> Backlog, Item closed -> Done, Pull request merged -> Done,
     Auto-add to project (is:issue is:open).
  4. Insights: Burn up chart over Sprint, grouped by Status.
EOF
