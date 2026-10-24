# 펌웨어 파일 받는 방법

[English](firmware-files.md)

Walkman Manager에는 펌웨어가 들어 있지 않습니다. 펌웨어 스튜디오는 직접 받은 파일로
이미지를 만듭니다. **파일 추가**를 누르거나 창에 끌어다 놓으면 됩니다. 앱은 파일을 있던
자리에서 읽고, [`kits/files.toml`](../../kits/files.toml)에 고정해 둔 크기와 해시로
확인합니다. 그래서 받다가 끊긴 파일이나 다른 빌드는 그렇다고 알려 주고 쓰지 않습니다.

앱이 직접 받는 파일은 라이선스상 누구나 다시 배포할 수 있는 것뿐입니다. TrebleDroid
이미지, Sony의 GPL 소스, walkman-zx507의 커널이 그렇습니다. 나머지 파일은 모두 각 배포처의 것이고, 이
문서는 배포처가 어디에 올려 두었는지 알려 드립니다.

## Sony 펌웨어 업데이트

NW-ZX500 시리즈 킷(Android 13)은 시리즈 `0000`의 4.06.00 업데이트를 씁니다. 킷의
패치가 이 빌드를 기준으로 만들어졌기 때문입니다. 순정으로 되돌릴 때 쓰는 것도 이
파일입니다. 다른 파일도 앱이 알아보고 이름을 보여 주지만, 쓰는 킷은 없고 만들 계획도
없습니다.

Sony는 이 파일들을 플레이어가 업데이트를 받아 가는 업데이트 서버에 두고 있습니다.

| 모델 | 시리즈 | 버전 | 파일 | 바이트 |
|---|---|---|---|---|
| NW-ZX500 | `0000` | 4.06.00 | [`NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0000/contents/0015/NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG) | 907,457,808 |
| NW-ZX500 | `0001` | 4.06.00 | [`NW-ZX500_0001_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0001/contents/0016/NW-ZX500_0001_V4_06_00_NW_WM_FW.UPG) | 910,338,016 |
| NW-ZX500 | `0002` | 4.06.00 | [`NW-ZX500_0002_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0002/contents/0015/NW-ZX500_0002_V4_06_00_NW_WM_FW.UPG) | 907,411,888 |
| NW-ZX500 | `0003` | 4.04.00 | [`NW-ZX500_0003_V4_04_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0003/contents/0013/NW-ZX500_0003_V4_04_00_NW_WM_FW.UPG) | 486,161,168 |
| NW-ZX500 | `0003` | 4.08.00 | [`NW-ZX500_0003_V4_08_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0003/contents/0014/NW-ZX500_0003_V4_08_00_NW_WM_FW.UPG) | 486,061,952 |
| NW-ZX500 | `0004` | 4.06.00 | [`NW-ZX500_0004_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0004/contents/0015/NW-ZX500_0004_V4_06_00_NW_WM_FW.UPG) | 907,426,160 |
| NW-ZX500 | `0005` | 4.06.00 | [`NW-ZX500_0005_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0005/contents/0015/NW-ZX500_0005_V4_06_00_NW_WM_FW.UPG) | 907,414,432 |
| NW-A100 | `0000` | 4.06.00 | [`NW-A100_0000_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-A100Series_0000/contents/0015/NW-A100_0000_V4_06_00_NW_WM_FW.UPG) | 906,889,168 |

시리즈 `0003`은 중국판입니다. Sony는 가끔 같은 자리에 파일을 다시 올립니다. 받은
파일이 더는 맞지 않으면 앱이 쓰지 않고 그렇다고 알려 줍니다.

업데이트는 플레이어마다 가진 키로 암호화되어 있습니다. 이 저장소와 앱 어디에도 그 키는
없습니다. 빌드하기 전에 스튜디오가 USB 디버깅으로 내 플레이어에서 키를 읽어 오고, 내
컴퓨터에만 보관합니다.

## Sony GPL 소스

커널을 내 컴퓨터에서 직접 빌드할 때만 필요하고, 앱에서 바로 받을 수 있습니다. Sony는
이 파일을 소스 코드 배포 사이트의
[NW-A105 페이지](https://oss.sony.net/Products/Linux/Audio/NW-A105_Ver20211130.html)에
올려 두었고, NW-ZX500 시리즈도 이 페이지에 함께 들어 있습니다. 파일은
`gpl_source.tgz`, 304,730,290바이트, MD5 `4ea368efdb62d167483d89e6ec60e2c3`입니다.

## NW-ZX500용 커널

NW-ZX500 시리즈 킷은 Android 13에 맞게 고친 Sony 4.14 커널을 따로 빌드해서 씁니다.
walkman-zx507이 이 커널을 `Image-zx507-4.14.78-1`이라는 릴리스로 공개하고, GPL에 따라 전체
소스(패치를 적용한 Sony 소스 트리, `.config`, 빌드 스크립트)를 그 옆에 함께 올려 둡니다.
스튜디오는 GSI처럼 커널 카드의 **받기**로 이 파일을 받습니다.

직접 빌드하려면 킷 저장소의 `kernel/build.sh`가 x86_64 Linux 컴퓨터에서 위의 Sony GPL 소스와
AOSP의 gcc 4.9 툴체인(스크립트가 받아 옴)으로 같은 커널을 만듭니다. 20분쯤 걸리고 몇 GB를
씁니다.

```
kernel/build.sh gpl_source.tgz kernel-out
```

`kernel-out/Image`를 다른 파일처럼 스튜디오에 추가하면 됩니다. 앱은 어느 쪽이든 파일 안의
버전 문자열 `4.14.78-g69dd21e`로 알아봅니다. 이 문자열이 Sony 커널과 같아야 플레이어의
Wi-Fi, 오디오, 저장장치 모듈이 올라갑니다.

## Fire 7용 LineageOS

NW-A50 시리즈 킷(Android 5.1, 플레이어 자체 시스템 위에서 컨테이너로 실행)은 Amazon
Fire 7(5세대, 코드명 `ford`)용 비공식 LineageOS 12.1을 바탕으로 합니다.
[AndroidFileHost](https://androidfilehost.com/?fid=4349826312261799008)의
`lineage-12.1-20200505-UNOFFICIAL-ford.zip`입니다. 앱은 MD5로 확인합니다.

## Fire 7용 Fire OS

NW-A50 시리즈 킷은 같은 태블릿용 Amazon Fire OS 5.3.6.4 업데이트도 씁니다. 그 부트
이미지에 있는 컨테이너의 `init`, `healthd`, 장치 규칙만 가져오고 나머지는 쓰지 않습니다.

파일은 `update-kindle-Fire_7_LC_5th_Gen-37.6.2.6_user_626549020.bin`,
664,431,377바이트입니다. Amazon은 이 파일을
`https://kindle-fire-updates.s3.amazonaws.com/YNbeTpooFBaqHGwW9t4OkFAzHt/update-kindle-Fire_7_LC_5th_Gen-37.6.2.6_user_626549020.bin`에
올려 두었지만 지금은 403으로 응답합니다. 인터넷 아카이브의 Wayback Machine에 이 주소의
사본이 남아 있습니다.

## NW-A50 백업

NW-A50 시리즈 킷은 WalkmanUI에 쓰는 Sony 화면, 이미지, 글꼴과 부트 애니메이션, 그리고
플레이어 업데이터가 패키지를 확인하는 키를 플레이어에서 직접 가져옵니다. 플레이어가 자기
드라이브에 쓰는 파티션 백업, 약 2.4GB의 `WMBACKUP` 폴더가 그 출처입니다. 백업에는 그
플레이어에만 있는 정보(시리얼 번호, Wi-Fi·Bluetooth 주소, 보정값)도 들어 있으니 내 컴퓨터에만
두고 다른 사람에게 넘기지 마세요.

지금 킷은 패치를 만든 기준 펌웨어인 Walkman One(Sony 빌드 BBDMP2, 2019-07-31)의 Sony
파일만 받습니다. Sony 순정 1.02의 백업은 무엇인지 알려 주고 받지 않습니다.

백업 만드는 방법:

1. 킷 저장소인 [walkman-a55](https://github.com/RedHatOnTop/walkman-a55)의
   `backup-upg/build.sh`가 패키지 두 개를 만듭니다. Walkman One을 쓰는
   플레이어용 `NW_WM_FW.UPG`와 Sony 순정 펌웨어용 `stock/NW_WM_FW.UPG`입니다. 다른
   펌웨어용 패키지는 플레이어가 거절하고, 아무 일도 일어나지 않습니다.
2. 내 플레이어에 맞는 것을 WALKMAN 드라이브 맨 위에 복사하고, 파일 관리자에서 드라이브를
   꺼내기 합니다. 케이블은 그대로 둡니다.
3. Rockbox의 `scsitool`로 플레이어 업데이트를 시작합니다:
   `scsitool -d /dev/sgN do_fw_upgrade`. `/dev/sgN`은 WALKMAN 드라이브의 첫 번째 장치입니다.
4. 플레이어가 다시 시작하면서 `WMBACKUP`을 쓰고(몇 분), 평소처럼 켜집니다.
5. `WMBACKUP` 폴더를 통째로 컴퓨터에 복사하고, 그 안의 `manifest.txt`를 스튜디오에
   추가합니다. 앱은 폴더의 이미지를 하나하나 플레이어가 백업할 때 잰 MD5와 맞춰 봅니다.

## TrebleDroid

NW-ZX500 시리즈 킷의 시스템 이미지는 TrebleDroid의 `ci-20230905` arm64 A/B vanilla
빌드입니다. 앱이 [GitHub 릴리스](https://github.com/TrebleDroid/treble_experimentations/releases/tag/ci-20230905)에서
바로 받습니다. 이미 가지고 있는 파일도 압축된 것(`.img.xz`)이든 푼 것이든 그대로 쓸 수
있습니다.
