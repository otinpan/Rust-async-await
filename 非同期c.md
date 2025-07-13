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

自作Executorでは
* Executorはpollできるタスクがあるかを逐一チェックする -> ポーリング式
* I/Oはスレッドを作ることで個別に対応 -> スレッドをブロックする、リソースを消費

Tokioでは  
epoll用のスレッドを1つ用意してそのスレッドの中で、イベント駆動式、ブロッキングのような動作をする。イベントの通知が来たらメインスレッドのExecutorに通知を送る。その場合、スレッド1つ分のブロッキングはしかたないよね、という感じ。



## イベント駆動式
epollを扱うスレッドを立てることによってイベント駆動式を実装する。
* epollに標準入力を登録する
* epollスレッドで標準入力が到着したことを検出したら、workerスレッドにTaskを通して通知する
* workerスレッドは標準入力を受け取り、入力に対して処理をする

epollが入力を待っている間、workerは他の処理をすることが出来る。また、workerはタスクが来た時だけ起こされるためポーリング式ではなくイベント駆動式になる。
```c
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <sys/epoll.h>
#include <string.h>
#include <errno.h>
#include <signal.h>
#include <stdbool.h>

#define MAX_LINE 1024

int epfd;
bool is_running = true;

typedef struct Task {
    int fd;
    struct Task *next;
} task_t;

task_t *task_head = NULL;
task_t *task_tail = NULL;

pthread_mutex_t task_mutex = PTHREAD_MUTEX_INITIALIZER;
pthread_cond_t task_cond = PTHREAD_COND_INITIALIZER;

void *worker_thread_func(void *arg) {
    while (is_running) {
        pthread_mutex_lock(&task_mutex);
        while (task_head == NULL && is_running) {
            pthread_cond_wait(&task_cond, &task_mutex);
        }

        if (!is_running) {
            pthread_mutex_unlock(&task_mutex);
            break;
        }

        task_t *t = task_head;
        if (t) {
            task_head = t->next;
            if (task_head == NULL) {
                task_tail = NULL;
            }
        }

        pthread_mutex_unlock(&task_mutex);

        if (t) {
            char buf[MAX_LINE] = {0};
            int n = read(t->fd, buf, sizeof(buf) - 1);

            if (n > 0) {
                int val = atoi(buf);
                int result = val + 10;
                printf("[worker thread] %d + 10 = %d\n", val, result);
            } else if (n == 0) {
                printf("[worker thread] fd %d closed by peer\n", t->fd);
                close(t->fd);
            } else {
                perror("read");
            }

            free(t);
        }
    }
    return NULL;
}

void *epoll_thread_func(void *arg) {
    struct epoll_event events[10];

    while (is_running) {
        int nfds = epoll_wait(epfd, events, 10, 1000);

        if (nfds < 0) {
            if (errno == EINTR) continue;
            perror("epoll_wait"); 
            break;
        } else if (nfds == 0) {
            continue;
        }

        for (int i = 0; i < nfds; i++) {
            int fd = events[i].data.fd;

            task_t *t = (task_t*)malloc(sizeof(task_t));
            t->fd = fd;
            t->next = NULL;

            pthread_mutex_lock(&task_mutex);
            if (task_tail) {
                task_tail->next = t;
                task_tail = t;
            } else {
                task_head = task_tail = t;
            }

            pthread_cond_signal(&task_cond);
            pthread_mutex_unlock(&task_mutex);

            printf("[epoll thread] queued FD %d for worker\n", fd); 
        }
    }
    return NULL;
}


void sigint_handler(int signum) {
    is_running = false;
    pthread_cond_broadcast(&task_cond);
}

int main() {
    signal(SIGINT, sigint_handler);

    epfd = epoll_create1(0);
    if (epfd < 0) {
        perror("epoll_create1");
        return 1;
    }

    struct epoll_event ev;
    ev.events = EPOLLIN;
    ev.data.fd = 0; // 標準入力を登録

    if (epoll_ctl(epfd, EPOLL_CTL_ADD, 0, &ev) < 0) {
        perror("epoll_ctl");
        return 1;
    }

    pthread_t epoll_tid, worker_tid;
    if (pthread_create(&epoll_tid, NULL, epoll_thread_func, NULL) != 0) {
        perror("pthread_create epoll");
        return 1;
    }
    if (pthread_create(&worker_tid, NULL, worker_thread_func, NULL) != 0) {
        perror("pthread_create worker");
        return 1;
    }

    while (is_running) {
        printf("[main thread] doing other work...\n");
        sleep(2);
    }

    pthread_join(epoll_tid, NULL);
    pthread_join(worker_tid, NULL);
    close(epfd);

    printf("Program terminated.\n");
    return 0;
}

```
出力
```
[main thread] doing other work...
[main thread] doing other work...
3
[worker thread] 3 + 10 = 13
4
[worker thread] 4 + 10 = 14
[main thread] doing other work...
5
[worker thread] 5 + 10 = 15
[main thread] doing other work...
```


epoll を使って標準入力を監視するスレッドを立てる

標準入力にデータが来たら Waker を叩く

Executor は queue を巡回するのではなく eventfd（もしくは pipe）による通知で起床する

poll() が Pending を返したらまた待機する

Executor に eventfd_fd を持たせる

Task.schedule() で eventfd に書き込む

epoll スレッドを Executor 初期化時に立てる

stdin 用の Future を実装する

```
+-------------------+
|    ユーザ入力     |
|    (ターミナル)   |
+---------+---------+
          |
          v
+---------+---------+
|   epoll_wait()    |   <--- epoll スレッドが待っている
+---------+---------+
          |
(データ来たら)
          |
          v
+---------+---------+
|    Waker.wake()   |   <--- Future に渡した waker が呼ばれる
+---------+---------+
          |
          v
+---------+---------+
| write(eventfd_fd) |   <--- eventfd に書き込み → Executor に通知
+---------+---------+
          |
          v
+---------+---------+
| Executor.run()    |   <--- eventfd で起床
+---------+---------+
          |
          v
+---------+---------+
| task.poll()       |   <--- Future の poll() が呼ばれる
+---------+---------+
          |
(Readyなら結果取得)
          |
          v
+---------+---------+
| 結果を出力        |
+-------------------+
```