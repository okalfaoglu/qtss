#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./push-github.sh "commit mesajı"
#   REPO_URL=https://github.com/okalfaoglu/qtss.git ./push-github.sh "commit mesajı"
#   BRANCH=main ./push-github.sh "commit mesajı"
#
# One-time auth (pick one):
#   1) SSH key with GitHub
#   2) gh auth login

REPO_URL="${REPO_URL:-https://github.com/okalfaoglu/qtss.git}"
BRANCH="${BRANCH:-main}"
COMMIT_MSG="${1:-auto: update project}"

if [[ -z "${REPO_URL}" ]]; then
  echo "REPO_URL boş olamaz."
  exit 1
fi

# Git repo yoksa başlat
if [[ ! -d .git ]]; then
  git init
fi

# Branch ayarı
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")"
if [[ "${CURRENT_BRANCH}" == "HEAD" || -z "${CURRENT_BRANCH}" ]]; then
  git checkout -b "${BRANCH}"
elif [[ "${CURRENT_BRANCH}" != "${BRANCH}" ]]; then
  git checkout -B "${BRANCH}"
fi

# Remote ayarı
if git remote get-url origin >/dev/null 2>&1; then
  git remote set-url origin "${REPO_URL}"
else
  git remote add origin "${REPO_URL}"
fi

# Tüm değişiklikleri stage et
git add -A

# Hassas dosyaları varsayılan olarak commit dışı bırak
for f in ".env" "web/.env" ".env.local" "web/.env.local" "credentials.json" "secrets.json"; do
  if git ls-files --error-unmatch "$f" >/dev/null 2>&1 || [[ -f "$f" ]]; then
    git reset -q HEAD -- "$f" 2>/dev/null || true
  fi
done

# Stage edilecek bir şey yoksa çık
if git diff --cached --quiet; then
  echo "Commitlenecek değişiklik yok."
  exit 0
fi

git commit -m "${COMMIT_MSG}"
git push -u origin "${BRANCH}"

echo "Tamam: ${BRANCH} -> ${REPO_URL}"
