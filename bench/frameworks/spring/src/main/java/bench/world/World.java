package bench.world;

import jakarta.persistence.Column;
import jakarta.persistence.Entity;
import jakarta.persistence.Id;
import jakarta.persistence.Table;

@Entity
@Table(name = "world")
public class World {

    @Id
    private Integer id;

    @Column(name = "randomnumber", nullable = false)
    private Integer randomNumber;

    protected World() {
    }

    public Integer getId() {
        return id;
    }

    public Integer getRandomNumber() {
        return randomNumber;
    }
}
