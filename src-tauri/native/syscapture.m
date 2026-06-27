// src-tauri/native/syscapture.m — native macOS system-audio + mic capture via ScreenCaptureKit.
//
// Zero-setup live capture: ScreenCaptureKit grabs the system audio (the call you hear) and,
// optionally, the microphone, with no BlackHole / Multi-Output / Aggregate devices. We expose a
// tiny C interface to Rust; each chunk is downmixed to mono float here and tagged with its real
// sample rate (the mic can arrive at its own native rate), and Rust resamples to 16 kHz for Whisper.
//
// Objective-C (not Swift) on purpose: the ObjC runtime is always present on macOS, so there's no
// Swift-runtime dylib to bundle (which is what made the Rust ScreenCaptureKit *crate* unshippable).
#import <CoreMedia/CoreMedia.h>
#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>

// Rust receives each chunk here: (ctx, mono f32 samples, frame count, sample rate Hz, is_mic).
// The pointer is only valid for the duration of the call — Rust copies immediately.
typedef void (*ember_audio_cb)(void *ctx, const float *mono, int frames, double rate, int is_mic);

@interface EmberCaptureOutput : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic, assign) ember_audio_cb cb;
@property(nonatomic, assign) void *ctx;
@end

@implementation EmberCaptureOutput
- (void)stream:(SCStream *)stream
    didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer
                   ofType:(SCStreamOutputType)type {
  if (type != SCStreamOutputTypeAudio && type != SCStreamOutputTypeMicrophone) return;
  if (!self.cb || !CMSampleBufferDataIsReady(sampleBuffer)) return;
  int is_mic = (type == SCStreamOutputTypeMicrophone) ? 1 : 0;

  // Real format: sample rate, channel count, interleaved vs planar.
  CMFormatDescriptionRef fmt = CMSampleBufferGetFormatDescription(sampleBuffer);
  const AudioStreamBasicDescription *asbd =
      fmt ? CMAudioFormatDescriptionGetStreamBasicDescription(fmt) : NULL;
  double rate = asbd ? asbd->mSampleRate : 48000.0;
  int channels = (asbd && asbd->mChannelsPerFrame > 0) ? (int)asbd->mChannelsPerFrame : 1;
  BOOL planar = asbd ? (asbd->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0 : YES;

  AudioBufferList abl;
  CMBlockBufferRef blockBuf = NULL;
  OSStatus st = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
      sampleBuffer, NULL, &abl, sizeof(abl), NULL, NULL, 0, &blockBuf);
  if (st != noErr || abl.mNumberBuffers == 0) {
    if (blockBuf) CFRelease(blockBuf);
    return;
  }

  if (planar) {
    int nb = (int)abl.mNumberBuffers;  // one buffer per channel
    int frames = (int)(abl.mBuffers[0].mDataByteSize / sizeof(float));
    if (frames > 0) {
      if (nb == 1) {
        self.cb(self.ctx, (const float *)abl.mBuffers[0].mData, frames, rate, is_mic);
      } else {
        float *mono = (float *)malloc((size_t)frames * sizeof(float));
        for (int i = 0; i < frames; i++) {
          float s = 0.0f;
          for (int b = 0; b < nb; b++) s += ((const float *)abl.mBuffers[b].mData)[i];
          mono[i] = s / (float)nb;
        }
        self.cb(self.ctx, mono, frames, rate, is_mic);
        free(mono);
      }
    }
  } else {
    // Interleaved: all channels packed in mBuffers[0].
    int total = (int)(abl.mBuffers[0].mDataByteSize / sizeof(float));
    int ch = channels;
    int frames = total / ch;
    const float *data = (const float *)abl.mBuffers[0].mData;
    if (frames > 0) {
      if (ch == 1) {
        self.cb(self.ctx, data, frames, rate, is_mic);
      } else {
        float *mono = (float *)malloc((size_t)frames * sizeof(float));
        for (int i = 0; i < frames; i++) {
          float s = 0.0f;
          for (int c = 0; c < ch; c++) s += data[i * ch + c];
          mono[i] = s / (float)ch;
        }
        self.cb(self.ctx, mono, frames, rate, is_mic);
        free(mono);
      }
    }
  }
  if (blockBuf) CFRelease(blockBuf);
}
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
}
@end

typedef struct {
  void *stream;  // CFBridgingRetain'd SCStream
  void *output;  // CFBridgingRetain'd EmberCaptureOutput
} EmberCapture;

// Start capture. Returns an opaque handle, or NULL on failure (message written to err_out).
void *ember_syscapture_start(int capture_mic, ember_audio_cb cb, void *ctx, char *err_out,
                             int err_len) {
  @autoreleasepool {
    __block SCShareableContent *content = nil;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [SCShareableContent
        getShareableContentWithCompletionHandler:^(SCShareableContent *c, NSError *e) {
          content = c;
          dispatch_semaphore_signal(sem);
        }];
    dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
    if (!content || content.displays.count == 0) {
      snprintf(err_out, err_len,
               "Screen Recording permission is needed. Enable Ember in System Settings -> "
               "Privacy & Security -> Screen Recording, then try again.");
      return NULL;
    }

    SCDisplay *display = content.displays.firstObject;
    SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
    SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
    config.capturesAudio = YES;
    config.sampleRate = 48000;
    config.channelCount = 1;
    config.excludesCurrentProcessAudio = YES;  // don't capture Ember's own output
    // We only consume audio; keep the (mandatory) video path tiny + slow.
    config.width = 2;
    config.height = 2;
    config.minimumFrameInterval = CMTimeMake(1, 1);
    if (capture_mic) {
      if (@available(macOS 15.0, *)) {
        config.captureMicrophone = YES;
      }
    }

    EmberCaptureOutput *output = [[EmberCaptureOutput alloc] init];
    output.cb = cb;
    output.ctx = ctx;

    SCStream *stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:output];
    dispatch_queue_t q = dispatch_queue_create("dev.ember.audiocapture", DISPATCH_QUEUE_SERIAL);
    NSError *addErr = nil;
    [stream addStreamOutput:output type:SCStreamOutputTypeAudio sampleHandlerQueue:q error:&addErr];
    if (capture_mic) {
      if (@available(macOS 15.0, *)) {
        [stream addStreamOutput:output
                           type:SCStreamOutputTypeMicrophone
             sampleHandlerQueue:q
                          error:&addErr];
      }
    }

    __block NSError *startErr = nil;
    dispatch_semaphore_t startSem = dispatch_semaphore_create(0);
    [stream startCaptureWithCompletionHandler:^(NSError *e) {
      startErr = e;
      dispatch_semaphore_signal(startSem);
    }];
    dispatch_semaphore_wait(startSem, DISPATCH_TIME_FOREVER);
    if (startErr) {
      snprintf(err_out, err_len, "could not start capture: %s",
               startErr.localizedDescription.UTF8String);
      return NULL;
    }

    EmberCapture *cap = (EmberCapture *)malloc(sizeof(EmberCapture));
    cap->stream = (void *)CFBridgingRetain(stream);
    cap->output = (void *)CFBridgingRetain(output);
    return cap;
  }
}

// Stop capture and release everything.
void ember_syscapture_stop(void *handle) {
  if (!handle) return;
  @autoreleasepool {
    EmberCapture *cap = (EmberCapture *)handle;
    SCStream *stream = (SCStream *)CFBridgingRelease(cap->stream);
    EmberCaptureOutput *output = (EmberCaptureOutput *)CFBridgingRelease(cap->output);
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [stream stopCaptureWithCompletionHandler:^(NSError *e) {
      dispatch_semaphore_signal(sem);
    }];
    dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
    (void)output;
    free(cap);
  }
}
