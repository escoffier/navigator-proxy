// use anyhow::Ok;
use netns::InpodNetns;
use std::{os::fd::OwnedFd, sync::Arc};
use nix::unistd::Pid;
use nix::sched::{setns, CloneFlags};
use std::result::Result::Ok;

pub mod netns;

fn new_netns(pid:Pid) -> OwnedFd {
    let mut new_netns: Option<OwnedFd> = None;
    std::thread::scope(|s| {
        s.spawn(|| {
            let res = nix::sched::unshare(CloneFlags::CLONE_NEWNET);
            if res.is_err() {
                return;
            }

            if let Ok(newns) =
                std::fs::File::open(format!("/proc/{}/ns/net", pid))
            {
                new_netns = Some(newns.into());
            }
        });
    });

    new_netns.expect("failed to create netns")
}


pub fn new_inpod_netns(pid:Pid) -> std::io::Result<InpodNetns> {
    let cur_netns: OwnedFd = InpodNetns::current()?;

    let workload_netns  = new_netns(pid);

    // let cur_netns = InpodNetns::current();
    // match cur_netns {
    //     Ok(cur_netns) => {
    //         let netns = InpodNetns::new(Arc::new(cur_netns) , workload_netns);
    //         netns.map(|n | n.into())
    //     },
    //     Err(err) => todo!(),
    // }


    let netns = InpodNetns::new(Arc::new(cur_netns) , workload_netns);
    netns.map(|n | n.into())
    // Ok(netns)
}
