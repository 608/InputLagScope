# InputLagScope

USB接続されたゲームコントローラーの入力遅延を、低レベルAPIと電気的なボタン入力を利用して計測するWindows用ソフトウェアです。

専用ファームウェア([InputLagScope-Firmware](https://github.com/608/InputLagScope-Firmware))を書き込んだRaspberry Pi Picoをコントローラーのボタン信号線に接続し、PCからシリアル経由で入力操作を再現します。<br>
入力を行った瞬間のタイムスタンプと、入力変化が反映されたレポートがPCに届いた時刻の差を取ることで、ハードウェアベースのテスターに相当する再現性の高い計測を行います。

<img width="1200" height="856" alt="image" src="https://github.com/user-attachments/assets/43786dd3-28f9-484a-b566-eb5b85e9dc02" />

## 注意点

計測対象のコントローラーは必ずCPU直結のUSBポートに接続してください。<br>
USBハブやチップセット経由のポートでは、経路上の遅延が計測結果に上乗せされます。

## 機能

- ボタン入力の遅延計測
- スティック入力の遅延計測
- スティック入力閾値の自動キャリブレーション
- ポーリングレートの計測
- 分布表示
- CLIでの計測

## 対応プロトコル

- XInput
- DirectInput
- DualShock 4
- DualSense
- Switch Pro Controller

## 必要なもの

- Windows PC
- ([InputLagScope-Firmware](https://github.com/608/InputLagScope-Firmware))を書き込んだRaspberry Pi Pico、またはRP2040ベースの開発ボード
- 計測対象のコントローラー

## 計測手順

1. Raspberry Pi PicoのGPIO25を対象デバイス上のボタン/スティック信号ピン、GNDを対象デバイス上のGNDに接続します。
2. 対象デバイスをPCに接続します。
2. InputLagScopeを起動し、対象デバイスを選択します。
3. 接続が正常であれば、計測の開始後に応答遅延が記録されます。

## ビルド

```powershell
npm install
npm run dev
npm run build
```

### CLI呼び出しでの計測方法

```powershell
InputLagScope.exe --headless-measure --samples 100 --input-type button
```

オプションの一覧は`--headless-measure --help`で確認できます。
