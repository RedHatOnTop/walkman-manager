echo @@props
getprop
echo @@version
cat /proc/version
echo @@meminfo
cat /proc/meminfo
echo @@power
for d in /sys/class/power_supply/*; do
  echo "== ${d##*/}"
  for f in type capacity status voltage_now temp health present online; do
    if [ -r "$d/$f" ]; then echo "$f=$(cat "$d/$f" 2>/dev/null)"; fi
  done
done
echo @@mounts
cat /proc/mounts
echo @@df
busybox df -k 2>/dev/null || df -k 2>/dev/null
echo @@asound
cat /proc/asound/cards
for s in /proc/asound/card*/pcm*p/sub0; do
  echo "== $s"
  cat "$s/status"
  echo "--"
  cat "$s/hw_params"
done
# A build without root keeps /proc/asound from the shell user; AudioFlinger still answers.
echo @@flinger
if ! cat /proc/asound/cards >/dev/null 2>&1 && command -v dumpsys >/dev/null 2>&1; then
  dumpsys -t 3 media.audio_flinger 2>/dev/null |
    grep -E '^Output thread |^  (Standby|Sample rate|HAL format|Channel count): '
fi
echo @@end
