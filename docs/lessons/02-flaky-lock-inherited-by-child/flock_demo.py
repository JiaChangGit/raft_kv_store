import fcntl, os, sys, time
path = sys.argv[1]
fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_CLOEXEC)
fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
print("parent: locked")
pid = os.fork()
if pid == 0:
    time.sleep(1.0)              # the window between fork and exec, stretched
    os.execv("/bin/true", ["true"])
os.close(fd)                     # like dropping the Wal
print("parent: closed its descriptor")
again = os.open(path, os.O_RDWR | os.O_CLOEXEC)
try:
    fcntl.flock(again, fcntl.LOCK_EX | fcntl.LOCK_NB)
    print("parent: re-locked immediately (no inheritance)")
except BlockingIOError:
    print("parent: re-lock FAILED while the child has not exec'd yet")
os.waitpid(pid, 0)
fcntl.flock(again, fcntl.LOCK_EX | fcntl.LOCK_NB)
print("parent: re-locked after the child exec'd and exited")
