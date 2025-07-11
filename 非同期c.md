# C言語の非同期処理
## 言葉の整理
### ブロッキング
* 処理が結果を得るまで、その場で待ち続ける
* 呼び出し元のスレッドやプロセスが止まる

### 非ブロッキング
* 処理を試みて、すぐ戻ってくる
* 呼び出し元は止まらず、自分で再度試すか他の処理を行う

| 項目 | ブロッキング                  | 非ブロッキング                      |
| -- | ----------------------- | ---------------------------- |
| 動き | 完了するまで戻らない              | すぐ戻る（未完了なら「まだ無い」）            |
| 例  | read(fd, …) → データ来るまで待つ | read(fd, …) → データ無ければ EAGAIN |


### 同期
* 完了まで、呼び出し元は次の処理を続けない
* 成功・失敗・完了の通知をその場で受け取る

### 非同期
* 処理を呼び出した時点で即戻り、結果は後で通知される
* 呼び出し元は他の作業ができる
* 完了時にはイベントやコールバックで結果が通知される

| 項目       | 同期                   | 非同期             |
| -------- | -------------------- | --------------- |
| 呼び出し後の動き | 終わるまで他のことをしない        | 呼び出した後に他のことができる |
| 結果の受け取り  | その場で返る               | 後で通知やコールバックで返る  |
| 例        | int result = func(); | func(callback); |


### ポーリング式
* イベントを逐一チェックする
* CPU効率は悪い

### イベント駆動式
* イベントはカーネルに通知させる
* CPU効率は良い

| 項目    | ポーリング式            | イベント駆動式              |
| ----- | ----------------- | -------------------- |
| 方法    | 定期的に自分で確認する       | カーネルに待たせ、通知を受ける      |
| CPU効率 | 悪い（無駄に動く）         | 良い（寝て待つ）             |
| 例     | select の 0秒タイムアウト | select の NULL タイムアウト |


| 処理例                   | ポーリング式？ | イベント駆動式？ | ブロッキング？    | 非ブロッキング？ | 同期？                   | 非同期？   |
| --------------------- | ------- | -------- | ---------- | -------- | --------------------- | ------ |
| while(read()==-1) {}  | ポーリング式  | ×        | ブロッキング     | ×        | 同期                    | ×      |
| select(NULL)          | ×       | イベント駆動式  | ブロッキング     | ×        | 同期                    | ×      |
| select(timeout=0)     | ポーリング式  | ×        | 非ブロッキングに近い | ×        | 同期                    | ×      |
| thread::spawn + sleep | ×       | イベント駆動風  | 内部はブロッキング  | ×        | ×                     | 非同期    |
| epoll\_wait           | ×       | イベント駆動式  | ブロッキング     | ×        | 同期的APIだけど、非同期的処理と相性良し | 両方に使える |
| async/await (Tokio)   | ×       | イベント駆動式  | 非ブロッキング    | 非ブロッキング  | 非同期                   | 非同期    |


## select 
```c
#include <stdio.h>
#include<sys/select.h>
#include<unistd.h>

int main(){
    fd_set rfds;
    int max_fd=0;

    FD_ZERO(&rfds);
    FD_SET(0,&rfds);

    max_fd=0;

    int ret=select(max_fd+1,&rfds,NULL,NULL,NULL); // データが到着するまで待つ
    if(ret<0){
        perror("select");
        return 1;
    }

    if (FD_ISSET(0,&rfds)){ // selectの結果を確認
        char buf[100];
        read(0,buf,sizeof(buf));
        printf("入力がありました: %s\n",buf);
    }
    return 0;
}
```
* `FD_ZERO`、`FD_SET`、`select()`を呼ぶ
* カーネルに
    - fd=0を監視してほしい
    - データが来たら教えて
と依頼する
* 標準入力にデータがまだないなら`select()`を呼んだスレッドはwait queueに入れられてスリープする。その際CPUは使わない
* データが到着したら
    - カーネルが`select()`をwakeする
    - `select()`が戻る
* `ret`にデータが到着したFDの数が記録される
* `FD_ISSET()`でどのFDかを確認する  

`select`が戻ってくるまで次の行に進めないで、止まっている。すなわちブロッキング、同期、イベント駆動式になる。

ほかのスレッドで`select`を作って、I/Oを管理しようとしたとしても、そのスレッドはブロックされてしまうため効率的ではない。

非ブロッキング、非同期風のselectを書く
```c
#include <stdio.h>
#include <sys/select.h>
#include <unistd.h>

int main() {
    fd_set rfds;
    struct timeval tv;
    int ret;

    while (1) {
        FD_ZERO(&rfds);
        FD_SET(0, &rfds);

        tv.tv_sec = 0;
        tv.tv_usec = 500000; // 0.5秒待つ

        ret = select(1, &rfds, NULL, NULL, &tv);
        if (ret < 0) {
            perror("select");
            return 1;
        }

        if (ret == 0) {
            printf("他の処理をしています...\n");
            // 他の処理がここで可能
            continue;
        }

        if (FD_ISSET(0, &rfds)) {
            char buf[100];
            read(0, buf, sizeof(buf));
            printf("入力がありました: %s\n", buf);
        }
    }
}

```

ここではselctのタイムアウトを短くすることで、他の処理を同時に動かせるようにしているが、CPU効率は良くない。  
以上のように、ずっと「まだか?」と確認し続けるような動作を**ポーリング式**と呼ぶ。

* `select(NULL)`はイベント駆動式だがブロッキング
    - シングルスレッドなら`select`が終わるまで次の処理ができないため同期処理になる
* 非同期処理風に書こうとすると`select(...,&tv)`となる
    - ブロック時間が`tv`となり、ノンブロッキング的に動作する
    - 何度も自分でループを回し入力が得られたか確認するためポーリング式になる
* 別スレッドで囲み、`select`が終わったらメインスレッドに通知する
    - 別スレッドがブロッキングする
    - スレッドが複数できて重い
    - イベント駆動式で非同期になる

`select`でノンブロッキング、非同期、イベント駆動式を書くことは難しい

## epoll
```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/epoll.h>
#include <fcntl.h>

int main() {
    int epfd = epoll_create1(0);
    if (epfd == -1) {
        perror("epoll_create1");
        return 1;
    }

    // 標準入力をノンブロッキングに設定
    int flags = fcntl(0, F_GETFL, 0);
    fcntl(0, F_SETFL, flags | O_NONBLOCK);

    struct epoll_event ev;
    ev.events = EPOLLIN;
    ev.data.fd = 0;

    if (epoll_ctl(epfd, EPOLL_CTL_ADD, 0, &ev) == -1) {
        perror("epoll_ctl");
        return 1;
    }

    struct epoll_event events[10];

    while (1) {
        int nfds = epoll_wait(epfd, events, 10, -1);
        if (nfds == -1) {
            perror("epoll_wait");
            break;
        }

        for (int i = 0; i < nfds; i++) {
            if (events[i].data.fd == 0) {
                char buf[1024];
                int n = read(0, buf, sizeof(buf)-1);
                if (n > 0) {
                    buf[n] = '\0';
                    printf("入力がありました: %s", buf);
                }
            }
        }

        // 他の処理もここに書ける
        printf("他の処理を同時に実行中...\n");
    }

    close(epfd);
    return 0;
}

```
入力
```
こんちは
```
出力
```
入力がありました: こんちは
他の処理を同時に実行中...
```

なにか入力したら他の処理をする。これは同期処理である。`select(...,NULL)`と同じようにイベント駆動方式である。イベントが起こったらepollは通知し、次の処理をする。それまではスレッドをブロッキングする。  

Tokioでは  
epoll用のスレッドを1つ用意してそのスレッドの中で、イベント駆動式、ブロッキングのような動作をする。イベントの通知が来たらメインスレッドのExecutorに通知を送る。その場合、スレッド1つ分のブロッキングはしかたないよね、という感じ。