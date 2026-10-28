import { defineComponent } from 'vue'
import { useData, withBase } from 'vitepress'

export default defineComponent({
  name: 'ModeVideo',
  props: {
    file: { type: String, default: '' },
    src: { type: String, default: '' },
    title: { type: String, required: true },
    description: { type: String, required: true },
  },
  setup(props) {
    const { lang } = useData()
    return () => {
      const source = withBase(props.src || `/video/${props.file}`)
      return (
        <figure class="mode-video">
          <video
            class="mode-video-player"
            src={source}
            controls
            playsinline
            preload="metadata"
            aria-label={props.title}
          >
            {lang.value === 'en-US' ? 'Your browser cannot play this MP4. ' : '当前浏览器无法播放 MP4。'}
            <a href={source}>{lang.value === 'en-US' ? 'Open the video directly' : '直接打开视频'}</a>
          </video>
          <figcaption>
            <strong>{props.title}</strong>
            <span>{props.description}</span>
            {props.src && <a href={source} target="_blank" rel="noopener noreferrer">
              {lang.value === 'en-US' ? 'Open the video directly' : '直接打开视频'}
            </a>}
          </figcaption>
        </figure>
      )
    }
  },
})
