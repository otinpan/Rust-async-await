## Rustの非同期処理

  - [sleep](#sleep)  
  - [ファイル読み込み](#ファイル読み込み)  
  - [並行サーバー](#並行サーバー)


Rustの非同期処理はCooperative (協調型) に近い仕組みです。Rustの非同期処理は非同期ランタイム (Tokio、async-stdなど) を用いて書かれることが多いです。これらの非同期ランタイムではスケジューラはユーザランドに存在し、タスクを管理・実行します。  
その振る舞いは、グリーンスレッドに似ています。すなわち：
* OSスレッドとは別にタスクという軽量な単位を扱う
* 複数のタスクを1つまたは少数のスレッドで切り替えて実行する
* スケジューラがユーザランドで動作する  

一旦、構文はおいておき、ここでは非同期ランタイムTokioを用いていくつかの基本的な非同期処理の動作を見ていきましょう。
### sleep
`tokio_runtime/sleep/src/main.rs`
```rust
use std::time;
use tokio::time as tokio_time;

#[tokio::main]
async fn main() {
    // 5秒スリープするタスク
    let five_secs_sleeper = tokio::spawn(async {
        println!("start 5secs sleep");
        let five_secs = time::Duration::from_secs(5);
        tokio_time::sleep(five_secs).await;
        println!("wake from 5secs sleep!");
    });

    // 2秒スリープするタスク
    let two_secs_sleeper = tokio::spawn(async {
        println!("start 2secs sleep");
        let two_secs = time::Duration::from_secs(2);
        tokio_time::sleep(two_secs).await;
        println!("wake from 2secs sleep!");
    });

    // Hello を表示するタスク
    let print_hello = tokio::spawn(async {
        println!("Hello");
    });

    let _ = tokio::join!(five_secs_sleeper, two_secs_sleeper, print_hello);
}


```
出力
```
start 5secs sleep
start 2secs sleep
Hello
wake from 2secs sleep!
wake from 5secs sleep!
```
`tokio::spawn(saync{...})`で新しいタスクが生成、起動されます。タスクの中の`.await`の部分でタスクは一時的に中断 (退避) されます。カーネルのタイマーを使い、指定した時間後に割り込みやイベントでタスクに通知が届きます。「タスクを再開しろ」と通知されたら、処理を再開します。  
スリープ中にCPUは他のタスクを処理できます。
### ファイル読み込み
`tokio_runtime/file_reader/src/main.rs`
```rust
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt};

#[tokio::main]
async fn main() -> io::Result<()> {
    // ファイル読み込みタスク
    let file_task = tokio::spawn(async {
        println!("start reading file");
        let mut file = File::open("example.txt").await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;
        println!("ファイル内容: {}", contents);
        io::Result::Ok(())
    });

    // Helloと出力するタスク
    let hello_task = tokio::spawn(async {
        println!("Hello");
        io::Result::Ok(())
    });

    let (res1, res2) = tokio::join!(file_task, hello_task);
    res1.unwrap()?;
    res2.unwrap()?;

    Ok(())
}

```
`tokio_runtime/file_reader/example.txt`
```
こんちはー
```
出力
```
start reading file
Hello
ファイル内容: こんちはー
```
ファイルを開く、ファイルを書き込みの部分で`.await`して他のタスクに処理を譲っています。その間`file_task`は一時的に中断 (退避) されます。ファイルを開いたり、ファイルを読み込んだらカーネルから通知が送られて、タスクは再開されます。

### 並行サーバー
このコードではクライアントから送られた入力を、サーバーがそのまま返します。
```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt}; 
use tokio::io;
use tokio::net::TcpListener; 

#[tokio::main] 
async fn main() -> io::Result<()> {
    // 10000番ポートでTCPリッスン 
    let listener = TcpListener::bind("127.0.0.1:10000").await.unwrap();

    loop {
        // TCPコネクションアクセプト 
        let (mut socket, addr) = listener.accept().await?;
        println!("accept: {}", addr);

        // 非同期タスク生成 
        tokio::spawn(async move {
            // バッファ読み書き用オブジェクト生成 
            let (r, w) = socket.split(); 
            let mut reader = io::BufReader::new(r);
            let mut writer = io::BufWriter::new(w);

            let mut line = String::new();
            loop {
                line.clear(); 
                // クライアントからの入力を非同期で処理
                match reader.read_line(&mut line).await { 
                    Ok(0) => { 
                        println!("closed: {}", addr);
                        return;
                    }
                    Ok(_) => {
                        print!("read: {}, {}", addr, line);
                        writer.write_all(line.as_bytes()).await.unwrap();
                        writer.flush().await.unwrap();
                    }
                    Err(e) => { // エラー
                        println!("error: {}, {}", addr, e);
                        return;
                    }
                }
            }
        });
    }
}
```
サーバ
```
accept: 127.0.0.1:44514
read: 127.0.0.1:44514, hello
```
クライアント
```
hello
hello
```

`listener.accept().await`で新しいTCP接続を非同期で待ちます。新しい接続がくるたびに、`tokio::spawn`で新しいタスクを起動します。つまり、接続がくるたびに非同期タスクが1つ増えるということです。  
それぞれのタスクは独立に動作し、以下の流れを繰り返します。
* クライアントからの入力を非同期で待つ
* 入力を受け取ったという通知がされる
* タスクを再開する
* 出力
* 再度、入力を非同期で待ち、待機状態になる  