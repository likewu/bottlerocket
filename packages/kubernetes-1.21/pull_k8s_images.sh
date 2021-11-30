#!/bin/sh
### 版本信息
K8S_VERSION=v1.21.6
ETCD_VERSION=3.2.24
DASHBOARD_VERSION=v1.8.3
FLANNEL_VERSION=v0.10.0-amd64
DNS_VERSION=1.14.8
PAUSE_VERSION=3.2
coredns_version=1.2.6
## 基本组件
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/kube-apiserver-amd64:$K8S_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/kube-controller-manager-amd64:$K8S_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/kube-scheduler-amd64:$K8S_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/openthings/k8s-gcr-io-kube-proxy-amd64:$K8S_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/etcd-amd64:$ETCD_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/pause-amd64:$PAUSE_VERSION
ctr i pull registry.cn-hangzhou.aliyuncs.com/google_containers/pause-amd64:$PAUSE_VERSION
#ctr i pull k8s.gcr.io/pause:$PAUSE_VERSION
### 网络
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-sidecar-amd64:$DNS_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-kube-dns-amd64:$DNS_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-dnsmasq-nanny-amd64:$DNS_VERSION
#docker pull quay.io/coreos/flannel:$FLANNEL_VERSION
#docker pull registry.cn-hangzhou.aliyuncs.com/openthings/k8s-gcr-io-coredns:$coredns_version
### 前端
#docker pull registry.cn-hangzhou.aliyuncs.com/google_containers/kubernetes-dashboard-amd64:$DASHBOARD_VERSION

## 修改tag
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/kube-apiserver-amd64:$K8S_VERSION k8s.gcr.io/kube-apiserver-amd64:$K8S_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/kube-controller-manager-amd64:$K8S_VERSION k8s.gcr.io/kube-controller-manager-amd64:$K8S_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/kube-scheduler-amd64:$K8S_VERSION k8s.gcr.io/kube-scheduler-amd64:$K8S_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/openthings/k8s-gcr-io-kube-proxy-amd64:$K8S_VERSION k8s.gcr.io/kube-proxy-amd64:$K8S_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/etcd-amd64:$ETCD_VERSION k8s.gcr.io/etcd-amd64:$ETCD_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/pause-amd64:$PAUSE_VERSION k8s.gcr.io/pause:$PAUSE_VERSION
ctr -n k8s.io i tag registry.cn-hangzhou.aliyuncs.com/google_containers/pause-amd64:$PAUSE_VERSION k8s.gcr.io/pause:$PAUSE_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-sidecar-amd64:$DNS_VERSION k8s.gcr.io/k8s-dns-sidecar-amd64:$DNS_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-kube-dns-amd64:$DNS_VERSION k8s.gcr.io/k8s-dns-kube-dns-amd64:$DNS_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-dnsmasq-nanny-amd64:$DNS_VERSION k8s.gcr.io/k8s-dns-dnsmasq-nanny-amd64:$DNS_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/google_containers/kubernetes-dashboard-amd64:$DASHBOARD_VERSION k8s.gcr.io/kubernetes-dashboard-amd64:$DASHBOARD_VERSION
#docker tag registry.cn-hangzhou.aliyuncs.com/openthings/k8s-gcr-io-coredns:$coredns_version  k8s.gcr.io/coredns:$coredns_version
## 删除镜像
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/kube-apiserver-amd64:$K8S_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/kube-controller-manager-amd64:$K8S_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/kube-scheduler-amd64:$K8S_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/openthings/k8s-gcr-io-kube-proxy-amd64:$K8S_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/etcd-amd64:$ETCD_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/pause-amd64:$PAUSE_VERSION
#ctr i rm registry.cn-hangzhou.aliyuncs.com/google_containers/pause-amd64:$PAUSE_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-sidecar-amd64:$DNS_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-kube-dns-amd64:$DNS_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/k8s-dns-dnsmasq-nanny-amd64:$DNS_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/google_containers/kubernetes-dashboard-amd64:$DASHBOARD_VERSION
#docker rmi registry.cn-hangzhou.aliyuncs.com/openthings/k8s-gcr-io-coredns:$coredns_version
